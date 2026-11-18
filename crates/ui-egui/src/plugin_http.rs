//! HTTPS for plug-in catalogs and GitHub releases, over the client TextureCraft checks for updates
//! with (its proxies and TLS settings).

use craft_update::http::Fetched;
use texturecraft_engine::plugin::craft_plugin::Fetch;

/// craft-update's client as craft-plugin's [`Fetch`].
struct UpdateHttp(craft_update::Http);

impl Fetch for UpdateHttp {
    fn get(&mut self, url: &str, max: usize) -> Result<Option<Vec<u8>>, String> {
        if !url.starts_with("https://api.github.com/") {
            return self.0.get_bytes(url, max);
        }
        match self.0.get_api(url, None)? {
            Fetched::Body { text, .. } if text.len() > max => Err(format!("GitHub's answer is larger than {max} bytes")),
            Fetched::Body { text, .. } => Ok(Some(text.into_bytes())),
            Fetched::NotFound => Ok(None),
            Fetched::RateLimited { .. } => Err("GitHub's API limit for this hour is used up: try again later".into()),
            Fetched::NotModified => Err("GitHub answered 304 Not Modified to a request without a cache".into()),
        }
    }
}

/// The HTTPS client for `Session::plugins` ([`texturecraft_engine::plugin::Runtime::set_fetch`]),
/// when one can be made.
pub fn http_fetch() -> Option<Box<dyn Fetch + Send>> {
    let agent = format!("TextureCraft/{}", texturecraft_engine::VERSION);
    match craft_update::Http::new(&agent) {
        Ok(http) => Some(Box::new(UpdateHttp(http))),
        Err(e) => {
            log::warn!("plug-in downloads: {e}");
            None
        }
    }
}
