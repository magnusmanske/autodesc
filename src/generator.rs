use crate::result::LanguageResult;
use crate::*;

pub trait LanguageGeneratorBase {
    fn new() -> Self;
}
pub trait LanguageGeneratorGeneric {}
pub trait LanguageGeneratorPerson {}

pub trait LanguageGenerator {
    fn new() -> Self;
    fn get_generic(&self) -> Box<dyn LanguageGeneratorGeneric>;
    fn get_person(&self) -> Option<Box<dyn LanguageGeneratorPerson>>;
    fn run(
        &self,
        item_id: &String,
        api: Arc<Api>,
        ec: Arc<Mutex<EntityContainer>>,
    ) -> Result<LanguageResult, Box<dyn (::std::error::Error)>> {
        let item = match ec.lock().unwrap().load_entity(&api, item_id) {
            Ok(item) => item.to_owned(),
            Err(e) => {
                return Err(From::from(format!(
                    "Could not load item {}: {}",
                    item_id, e
                )))
            }
        };
        if item.has_target_entity("P31", "Q5") {
            // TODO human
        }

        Ok(LanguageResult::Bold(Box::new(LanguageResult::Text(
            "blah".to_string(),
        ))))
    }
}
