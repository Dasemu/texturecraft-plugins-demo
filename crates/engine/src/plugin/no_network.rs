//! The HTTPS client of sessions without one.

use craft_plugin::fetch::Fetch;

/// Longest part of a URL an error repeats.
const SHOWN_URL_CHARS: usize = 120;

/// A fetcher for sessions without network access: `file://` catalogs still read.
pub(super) struct NoNetwork;

impl Fetch for NoNetwork {
    fn get(&mut self, url: &str, _max: usize) -> Result<Option<Vec<u8>>, String> {
        let shown: String = url.chars().take(SHOWN_URL_CHARS).collect();
        Err(format!("there is no network access here to read {shown}: install the plug-in from a file, or in the TextureCraft app"))
    }
}
