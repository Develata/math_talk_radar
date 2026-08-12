//! Source adapters (§13). Parsers turn `FetchedDocument`s into `EventStub`s
//! and `EventCandidate`s. No adapter performs network I/O.
//!
//! Adapter priority (§P-5): official JSON/API → RSS/Atom/JSON Feed → ICS →
//! JSON-LD Event → site-specific HTML selectors → generic HTML fallback.
#![forbid(unsafe_code)]

pub mod helpers;
pub mod html_config;
pub mod html_generic;
pub mod ics;
pub mod indico;
pub mod jsonld;
pub mod rss;

pub(crate) mod sites;

use radar_core::AdapterKind;
use radar_core::SourceAdapter;

/// Select the default [`SourceAdapter`] for a source's declared adapter kind.
/// `None` falls back to the generic HTML adapter (last resort, §P-5).
pub fn default_adapter(kind: AdapterKind) -> Box<dyn SourceAdapter> {
    match kind {
        AdapterKind::Rss => Box::new(rss::RssAdapter),
        AdapterKind::Ics => Box::new(ics::IcsAdapter),
        AdapterKind::JsonLd => Box::new(jsonld::JsonLdAdapter),
        AdapterKind::Indico => Box::new(indico::IndicoAdapter),
        AdapterKind::HtmlConfig => Box::new(html_config::HtmlConfigAdapter),
        AdapterKind::HtmlGeneric | AdapterKind::None => Box::new(html_generic::HtmlGenericAdapter),
    }
}
