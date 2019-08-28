use autodesc::english::*;
use autodesc::generator::*;
use mediawiki::api::Api;
use std::sync::{Arc, Mutex};
use wikibase::entity_container::EntityContainer;

fn main() {
    let wd: Arc<Api> = Arc::new(Api::new("https://www.wikidata.org/w/api.php").unwrap());
    let ec: Arc<Mutex<EntityContainer>> = Arc::new(Mutex::new(EntityContainer::new()));
    let en = GeneratorEn::new();
    let result = dbg!(en.run(&"Q42".to_string(), wd.clone(), ec.clone()).unwrap());
    println!("---\n{}\n---", result.render_text());
    println!("{}\n---", result.render_html());
}

#[cfg(test)]
mod tests {
    //use super::*;
}
