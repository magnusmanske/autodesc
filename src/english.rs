use crate::generator::*;

// GeneratorGenericEn
pub struct GeneratorGenericEn {}

impl LanguageGeneratorBase for GeneratorGenericEn {
    fn new() -> Self {
        GeneratorGenericEn {}
    }
}

impl LanguageGeneratorGeneric for GeneratorGenericEn {}

// GeneratorEn
pub struct GeneratorEn {}

impl LanguageGenerator for GeneratorEn {
    fn new() -> Self {
        Self {}
    }

    fn get_generic(&self) -> Box<dyn LanguageGeneratorGeneric> {
        Box::new(GeneratorGenericEn::new())
    }

    fn get_person(&self) -> Option<Box<dyn LanguageGeneratorPerson>> {
        None
    }
}
