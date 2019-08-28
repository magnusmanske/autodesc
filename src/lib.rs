//extern crate config;
extern crate mediawiki;
//#[macro_use]
extern crate lazy_static;
extern crate regex;
//#[macro_use]
extern crate serde_json;

use mediawiki::api::Api;
use std::sync::{Arc, Mutex};
use wikibase::entity_container::EntityContainer;
/*
use regex::Regex;
use std::env;
use std::io;
use std::io::prelude::*;
use std::thread;
use std::time::Duration;
*/

pub mod english;
pub mod generator;
pub mod result;

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
