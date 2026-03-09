/// Options passed to the description generator.
#[derive(Debug, Clone)]
pub struct DescOptions {
    pub q: String,
    pub lang: String,
    pub links: String,
    pub linktarget: String,
    pub redlinks: String,
    pub fallback: String,
    pub mode: String,
}

impl Default for DescOptions {
    fn default() -> Self {
        Self {
            q: String::new(),
            lang: "en".to_string(),
            links: "wikidata".to_string(),
            linktarget: String::new(),
            redlinks: String::new(),
            fallback: String::new(),
            mode: "short".to_string(),
        }
    }
}
