#[derive(Debug, Clone, PartialEq)]
pub enum LanguageResult {
    Text(String),
    Symbol(String),
    Link((String, Box<LanguageResult>)),
    Bold(Box<LanguageResult>),
    Italics(Box<LanguageResult>),
    Group(Vec<LanguageResult>),
    List((String, Vec<LanguageResult>)),
    Reference(Box<LanguageResult>),
}

impl LanguageResult {
    pub fn render_text(&self) -> String {
        match self {
            Self::Text(s) => s.clone(),
            Self::Symbol(s) => s.clone(),
            Self::Link((_url, r)) => r.render_text(),
            Self::Bold(r) => r.render_text(),
            Self::Italics(r) => r.render_text(),
            Self::Group(v) => {
                let mut s = "".to_string();
                for r in v {
                    match r {
                        Self::Symbol(_) => {}
                        _ => s += &" ".to_string(),
                    }
                    s += &r.render_text();
                }
                s
            }
            Self::List((last_separator, v)) => {
                let mut parts: Vec<String> = v.iter().map(|r| r.render_text()).collect();
                if parts.is_empty() {
                    return "".to_string();
                }
                if parts.len() == 1 {
                    return parts.get(0).unwrap().to_string();
                }
                let last = parts.pop().unwrap();
                parts.join(", ") + " " + last_separator + " " + &last
            }
            Self::Reference(_r) => "".to_string(), // Don't show reference
        }
    }

    pub fn render_html(&self) -> String {
        match self {
            Self::Text(s) => s.clone(),
            Self::Symbol(s) => s.clone(),
            Self::Link((url, r)) => {
                "<a href='".to_string() + &url + "'>" + &r.render_text() + "</a>"
            }
            Self::Bold(r) => "<strong>".to_string() + &r.render_text() + "</strong>",
            Self::Italics(r) => "<em>".to_string() + &r.render_text() + "</em>",
            Self::Group(v) => {
                let mut s = "".to_string();
                for r in v {
                    match r {
                        Self::Symbol(_) => {}
                        _ => s += &" ".to_string(),
                    }
                    s += &r.render_text();
                }
                s
            }
            Self::List((last_separator, v)) => {
                let mut parts: Vec<String> = v.iter().map(|r| r.render_text()).collect();
                if parts.is_empty() {
                    return "".to_string();
                }
                if parts.len() == 1 {
                    return parts.get(0).unwrap().to_string();
                }
                let last = parts.pop().unwrap();
                parts.join(", ") + " " + last_separator + " " + &last
            }
            Self::Reference(r) => "<reference>".to_string() + &r.render_text() + "</reference>", // TODO
        }
    }
}
