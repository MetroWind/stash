use std::collections::HashMap;
use std::path::PathBuf;
use log::error as log_error;
use log::{info, debug};

use chrono::prelude::*;
use warp::{Filter, Reply};
use warp::http::status::StatusCode;
use warp::redirect::see_other;
use warp::reply::Response;
use tera::Tera;

// use crate::rterr;
use crate::error::Error;
use crate::data;
use crate::session::getSession;
use crate::config::Configuration;

trait ToResponse
{
    fn toResponse(self) -> Response;
}

fn filterEncodeURI(uri: &tera::Value, _: &HashMap<String, tera::Value>) ->
    tera::Result<tera::Value>
{
    let result = urlencoding::encode(uri.as_str().ok_or_else(
        || tera::Error::msg("URI should be string"))?);
    Ok(tera::Value::from(result))
}

fn getWebpage(uri: &str) -> Result<String, Error>
{
    let res = ureq::get(&uri)
        .set("User-Agent", "Mozilla/5.0 (X11; Linux i686; rv:98.0) Gecko/20100101 Firefox/98.0")
        .call().map_err(
        |e| rterr!("Failed to query {}: {}", uri, e))?;
    if res.status() != 200
    {
        return Err(rterr!("Query got {}", res.status()));
    }
    res.into_string().map_err(
        |_| rterr!("Failed to get response body"))
}

fn getTitle(html_str: &str) -> Option<&str>
{
    if let Some(begin) = html_str.find("<title>")
    {
        if let Some(end) = html_str.find("</title>")
        {
            return Some(html_str[begin + 7..end].trim());
        }
    }
    None
}

fn isTweet(uri: &str) -> bool
{
    regex::Regex::new(r"https?://(www\.)?twitter\.com/[^/]+/status/.*").unwrap()
        .is_match(uri)
}

fn getTitleForTweet(tweet_uri: &str) -> Result<String, Error>
{
    let data: serde_json::Value = ureq::get(&format!("https://publish.twitter.com/oembed?url={}",
                                                     tweet_uri))
        .set("User-Agent", "Mozilla/5.0 (X11; Linux i686; rv:98.0) Gecko/20100101 Firefox/98.0")
        .call().map_err(
            |e| rterr!("Failed to query Twitter oEmbed API: {}", e))?
        .into_json().map_err(
            |_| rterr!("Invalid oEmbed response"))?;
    let html = data["html"].as_str().ok_or_else(
        || rterr!("Failed to get HTML from oEmbed response"))?;
    let the_match = regex::Regex::new(
        r".*<blockquote[^>]*><p[^>]*>(.+)</p>.*<a .*</a></blockquote>.*")
        .unwrap().captures(html).ok_or_else(
            || rterr!("Failed to match HTML"))?.get(1).ok_or_else(
            || rterr!("Failed to capture tweet content"))?;
    Ok(the_match.as_str().to_owned())
}

impl ToResponse for Result<String, Error>
{
    fn toResponse(self) -> Response
    {
        match self
        {
            Ok(s) => warp::reply::html(s).into_response(),
            Err(e) => {
                log_error!("{}", e);
                warp::reply::with_status(
                e.to_string(), StatusCode::INTERNAL_SERVER_ERROR)
                    .into_response()
            },
        }
    }
}

impl ToResponse for Result<Response, Error>
{
    fn toResponse(self) -> Response
    {
        match self
        {
            Ok(s) => s.into_response(),
            Err(e) => {
                log_error!("{}", e);
                warp::reply::with_status(
                e.to_string(), StatusCode::INTERNAL_SERVER_ERROR)
                    .into_response()
            }
        }
     }
}

pub struct App
{
    data_source: data::DataManager,
    templates: Tera,
    config: Configuration,
}

fn handleIndex(data_source: &data::DataManager, templates: &Tera) ->
    Result<String, Error>
{
    let user = getSession(data_source)?.user;
    let entries = data_source.getEntries(&user, 0, 50)?;
    let mut context = tera::Context::new();
    context.insert("entries", &entries);
    templates.render("index.html", &context).map_err(
        |e| rterr!("Failed to render template index.html: {}", e))
}

fn handleAdd(templates: &Tera) -> Result<String, Error>
{
    if let Ok(s) = templates.render("add.html", &tera::Context::new())
    {
        Ok(s)
    }
    else
    {
        Err(rterr!("Failed to render template add.html"))
    }
}

fn handleAddSubmit(data_source: &data::DataManager,
                   mut form_data: HashMap<String, String>) ->
    Result<Response, Error>
{
    let uri_str = form_data.remove("url").unwrap();
    let text = getWebpage(&uri_str)?;
    let title: String = if isTweet(&uri_str)
    {
        getTitleForTweet(&uri_str)?
    }
    else if let Some(t) = getTitle(&text)
    {
        t.to_owned()
    }
    else
    {
        debug!("No title found in HTML: {}", text);
        "(No title)".to_owned()
    };
    let user = getSession(data_source)?.user;
    debug!("Adding entry for {}...", uri_str);
    let entry = data::Entry {
        uri: uri_str,
        title: title,
        time_add: Utc::now(),
    };
    data_source.addEntry(&user, entry)?;
    Ok(see_other(warp::http::Uri::from_static("/")).into_response())
}

fn handleRead(data_source: &data::DataManager, uri: String) ->
    Result<Response, Error>
{
    debug!("Reading entry at {}...", uri);
    let uri = urlencoding::decode(&uri).map_err(
        |_| rterr!("Failed to decode URI: {}", uri))
        .map(|v| v.into_owned())?;
    let user = getSession(data_source)?.user;
    let entry = data_source.findEntryByURI(&user, &uri)?.ok_or_else(
        || rterr!("Entry not found @ {}", uri))?;
    data_source.readEntry(&user, &entry)?;
    // Redirect to the URI, but ask the browser to not cache it. If
    // the browser caches it, and the user read the URI again (after
    // adding the URI again), the browser may skip the query and
    // directly load the URI from cache.
    Ok(warp::reply::with_header(
        warp::redirect(uri.parse::<warp::http::Uri>().unwrap()),
        "Cache-Control", "no-cache").into_response())
}

impl App
{
    pub fn new(config: Configuration) -> Self
    {
        Self {
            data_source: data::DataManager::newWithFilename(&config.db_path),
            templates: Tera::default(),
            config: config,
        }
    }

    pub fn init(&mut self) -> Result<(), Error>
    {
        self.data_source.connect()?;
        self.data_source.init()?;
        let template_path = PathBuf::from(&self.config.data_dir)
            .join("templates").canonicalize()
            .map_err(|_| rterr!("Invalid template dir"))?
            .join("**").join("*");
        info!("Template dir is {}", template_path.display());
        let template_dir = template_path.to_str().ok_or_else(
                || rterr!("Invalid template path"))?;
        self.templates = Tera::new(template_dir).map_err(
            |e| rterr!("Failed to compile templates: {}", e))?;
        self.templates.register_filter("urlencode", filterEncodeURI);
        for name in self.templates.get_template_names()
        {
            debug!("Found template {}.", name);
        }
        Ok(())
    }

    pub async fn serve(self) -> Result<(), Error>
    {
        let static_dir = PathBuf::from(&self.config.data_dir).join("static");
        info!("Static dir is {}", static_dir.display());
        let statics = warp::path("static").and(warp::fs::dir(static_dir));

        let temp = self.templates.clone();
        let manager = self.data_source.clone();
        let index = warp::get().and(warp::path::end()).map(move || {
            handleIndex(&manager, &temp).toResponse()
        });

        let temp = self.templates.clone();
        let add = warp::get().and(warp::path("add")).map(move || {
            handleAdd(&temp).toResponse()
        });

        let manager = self.data_source.clone();
        let add_submit = warp::post().and(warp::path("add"))
            .and(warp::body::content_length_limit(1024 * 32))
            .and(warp::body::form())
            .map(move |form: HashMap<String, String>| {
            handleAddSubmit(&manager, form).toResponse()
        });

        let manager = self.data_source.clone();
        let read = warp::get().and(warp::path("read"))
            .and(warp::path::param())
            .map(move |uri: String| {
            handleRead(&manager, uri).toResponse()
        });

        info!("Listening at {}:{}...", self.config.listen_address,
              self.config.listen_port);

        warp::serve(statics.or(index).or(add).or(add_submit).or(read)).run(
            std::net::SocketAddr::new(
                self.config.listen_address.parse().map_err(
                    |_| rterr!("Invalid listen address: {}",
                               self.config.listen_address))?,
                self.config.listen_port)).await;
        Ok(())
    }
}
