/// Options passed to the description generator.
use wikimisc::entity_id::ItemId;

use crate::lang_type::Lang;

#[derive(Debug, Clone)]
pub struct DescOptions {
    pub q: ItemId,
    pub lang: Lang,
    pub links: String,
    pub linktarget: String,
    pub redlinks: String,
    pub fallback: String,
    pub mode: String,
}

impl Default for DescOptions {
    fn default() -> Self {
        Self {
            q: ItemId::new(0), // placeholder; always overwritten
            lang: Lang::default(),
            links: "wikidata".to_string(),
            linktarget: String::new(),
            redlinks: String::new(),
            fallback: String::new(),
            mode: "short".to_string(),
        }
    }
}
