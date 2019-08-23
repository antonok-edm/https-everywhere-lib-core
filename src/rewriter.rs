use url::Url;
use std::error::Error;
use regex::Regex;

use crate::{RuleSet, RuleSets, Storage};
#[cfg(all(test,feature="add_rulesets"))]
use crate::rulesets::tests as rulesets_tests;

/// A RewriteAction is used to indicate an action to take, returned by the rewrite_url method on
/// the Rewriter struct
#[derive(Debug)]
#[derive(PartialEq)]
pub enum RewriteAction {
    CancelRequest,
    NoOp,
    RewriteUrl(String),
}

/// A Rewriter provides an abstraction layer over RuleSets and Storage, providing the logic for
/// rewriting URLs
pub struct Rewriter<'a> {
    rulesets: &'a RuleSets,
    storage: &'a Storage,
}

impl<'a> Rewriter<'a> {
    /// Returns a rewriter with the rulesets and storage engine specified
    ///
    /// # Arguments
    ///
    /// * `rulesets` - An instance of RuleSets for rewriting URLs
    /// * `storage` - A storage object to query current state
    pub fn new(rulesets: &'a RuleSets, storage: &'a Storage) -> Rewriter<'a> {
        Rewriter {
            rulesets,
            storage,
        }
    }

    /// Return a RewriteAction wrapped in a Result when given a URL.  This action should be
    /// respected by the implementation using the library
    ///
    /// # Arguments
    ///
    /// * `url` - A URL to determine the action for
    pub fn rewrite_url(&self, url: &String) -> Result<RewriteAction, Box<dyn Error>> {
        if let Some(false) = self.storage.get_bool(String::from("global_enabled")){
            return Ok(RewriteAction::NoOp);
        }

        let mut url = Url::parse(url)?;
        if let Some(hostname) = url.host_str() {
            let mut hostname = hostname.trim_end_matches('.');
            if hostname.len() == 0 {
                hostname = ".";
            }
            let hostname = hostname.to_string();

            let mut should_cancel = false;
            let http_nowhere_on = self.storage.get_bool(String::from("http_nowhere_on"));
            if let Some(true) = http_nowhere_on {
                if url.scheme() == "http" || url.scheme() == "ftp" {
                    let num_localhost = Regex::new(r"^127(\.[0-9]{1,3}){3}$").unwrap();
                    if !hostname.ends_with(".onion") &&
                        hostname != "localhost".to_string() &&
                        !num_localhost.is_match(&hostname) &&
                        hostname != "0.0.0.0".to_string() &&
                        hostname != "[::1]".to_string() {
                        should_cancel = true;
                    }
                }
            }
            let mut using_credentials_in_url = false;
            let tmp_url = url.clone();
            if url.username() != "" || url.password() != None {
                using_credentials_in_url = true;
                url.set_username("").unwrap();
                url.set_password(None).unwrap();
            }

            let mut new_url: Option<Url> = None;

            let mut apply_if_active = |ruleset: &RuleSet| {
                if ruleset.active && new_url.is_none() {
                    new_url = match ruleset.apply(url.as_str()) {
                        None => None,
                        Some(url_str) => Some(Url::parse(&url_str).unwrap())
                    };
                }
            };


            for ruleset in self.rulesets.potentially_applicable(&hostname) {
                if let Some(scope) = (*ruleset.scope).clone() {
                    let scope_regex = Regex::new(&scope).unwrap();
                    if scope_regex.is_match(url.as_str()) {
                        apply_if_active(&ruleset);
                    }
                } else {
                    apply_if_active(&ruleset);
                }
            }

            if using_credentials_in_url {
                match &mut new_url {
                    None => {
                        url.set_username(tmp_url.username()).unwrap();
                        url.set_password(tmp_url.password()).unwrap();
                    },
                    Some(url) => {
                        url.set_username(tmp_url.username()).unwrap();
                        url.set_password(tmp_url.password()).unwrap();
                    }
                }
            }

            if let Some(true) = http_nowhere_on {
                if should_cancel {
                    if new_url.is_none() {
                        return Ok(RewriteAction::CancelRequest);
                    }
                }

                // Cancel if we're about to redirect to HTTP or FTP in EASE mode
                if let Some(url) = &new_url {
                    if url.as_str().starts_with("http:") ||
                       url.as_str().starts_with("ftp:") {
                        return Ok(RewriteAction::CancelRequest);
                    }
                }
            }

            if let Some(url) = new_url {
                info!("rewrite_url returning redirect url: {}", url.as_str());
                Ok(RewriteAction::RewriteUrl(url.as_str().to_string()))
            } else {
                Ok(RewriteAction::NoOp)
            }
        } else {
            Ok(RewriteAction::NoOp)
        }
    }
}

#[cfg(all(test,feature="add_rulesets"))]
mod tests {
    use super::*;

    struct DefaultStorage;
    impl Storage for DefaultStorage {
        fn get_int(&self, _key: String) -> Option<usize> { Some(5) }
        fn set_int(&self, _key: String, _value: usize) {}
        fn get_string(&self, _key: String) -> Option<String> { Some(String::from("test")) }
        fn set_string(&self, _key: String, _value: String) {}
        fn get_bool(&self, key: String) -> Option<bool> {
            if key == String::from("http_nowhere_on") {
                Some(false)
            } else {
                Some(true)
            }
        }
        fn set_bool(&self, _key: String, _value: bool) {}
    }

    struct HttpNowhereOnStorage;
    impl Storage for HttpNowhereOnStorage {
        fn get_int(&self, _key: String) -> Option<usize> { Some(5) }
        fn set_int(&self, _key: String, _value: usize) {}
        fn get_string(&self, _key: String) -> Option<String> { Some(String::from("test")) }
        fn set_string(&self, _key: String, _value: String) {}
        fn get_bool(&self, _key: String) -> Option<bool> { Some(true) }
        fn set_bool(&self, _key: String, _value: bool) {}
    }

    #[test]
    fn rewrite_url() {
        let mut rs = RuleSets::new();
        rulesets_tests::add_mock_rulesets(&mut rs);

        let rw = Rewriter::new(&rs, &DefaultStorage);

        assert_eq!(
            rw.rewrite_url(&String::from("http://freerangekitten.com/")).unwrap(),
            RewriteAction::RewriteUrl(String::from("https://freerangekitten.com/")));

        assert_eq!(
            rw.rewrite_url(&String::from("http://fake-example.com/")).unwrap(),
            RewriteAction::NoOp);
    }

    #[test]
    fn rewrite_url_http_nowhere_on() {
        let mut rs = RuleSets::new();
        rulesets_tests::add_mock_rulesets(&mut rs);

        let rw = Rewriter::new(&rs, &HttpNowhereOnStorage);

        assert_eq!(
            rw.rewrite_url(&String::from("http://freerangekitten.com/")).unwrap(),
            RewriteAction::RewriteUrl(String::from("https://freerangekitten.com/")));

        assert_eq!(
            rw.rewrite_url(&String::from("http://fake-example.com/")).unwrap(),
            RewriteAction::CancelRequest);

        assert_eq!(
            rw.rewrite_url(&String::from("http://fake-example.onion/")).unwrap(),
            RewriteAction::NoOp);

        assert_eq!(
            rw.rewrite_url(&String::from("http://fake-example.onion..../")).unwrap(),
            RewriteAction::NoOp);
    }

    #[test]
    fn rewrite_exclusions() {
        let mut rs = RuleSets::new();
        rulesets_tests::add_mock_rulesets(&mut rs);

        let rw = Rewriter::new(&rs, &DefaultStorage);

        assert_eq!(
            rw.rewrite_url(&String::from("http://chart.googleapis.com/")).unwrap(),
            RewriteAction::NoOp);

        assert_eq!(
            rw.rewrite_url(&String::from("http://chart.googleapis.com/123")).unwrap(),
            RewriteAction::RewriteUrl(String::from("https://chart.googleapis.com/123")));
    }

    #[test]
    fn rewrite_with_credentials() {
        let mut rs = RuleSets::new();
        rulesets_tests::add_mock_rulesets(&mut rs);

        let rw = Rewriter::new(&rs, &DefaultStorage);

        assert_eq!(
            rw.rewrite_url(&String::from("http://eff:techprojects@chart.googleapis.com/123")).unwrap(),
            RewriteAction::RewriteUrl(String::from("https://eff:techprojects@chart.googleapis.com/123")));
    }
}
