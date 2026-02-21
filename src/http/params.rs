use std::collections::HashMap;

use regex::Regex;
use lazy_static::lazy_static;

// 支持 :param? 可选参数 和 * 通配符
const PATH_PARAMS: &str = r"(?s)(?::([^/\.?]+)\??)|(\*)";

lazy_static! {
    static ref PATH_PARAMS_RE: Regex = Regex::new(PATH_PARAMS).unwrap();
}
/// URL 参数结构
#[derive(Debug, Clone)]
pub struct Params {
    /// 原始请求 URL，包括 query
    pub url: String,
    /// Path 参数，例如 /user/:id -> {"id": "123"}
    pub data: Option<HashMap<String, String>>,
    /// Query 参数，例如 ?active=true -> {"active": "true"}
    pub query: HashMap<String, Vec<String>>,
    pub form: Option<HashMap<String, Vec<String>>>,
    pub pattern: String,
}

impl Params {
    pub fn new(url: String, pattern: String) -> Self {
        let (_, query) = match url.split_once('?') {
            Some((path_part, query_part)) => { (path_part, Self::parse_pairs(query_part)) }
            None => (url.as_str(), HashMap::new()),
        };
        Self { url, data: None, query, pattern, form: None }
    }

    pub fn parse_pairs(pairs: &str) -> HashMap<String, Vec<String>> {
        let mut map: HashMap<String, Vec<String>> = HashMap::new();
        for (k, v) in form_urlencoded::parse(pairs.as_bytes()) {
            map.entry(k.into_owned()).or_default().push(v.into_owned());
        }
        map
    }

    /// 根据 URL 提取 query params
    /// 支持数组参数
    // fn parse_query(url: &str) -> HashMap<String, Vec<String>> {
    //     url.split_once('?')
    //         .map(|(_, qs)| Self::parse_pairs(qs))
    //         .unwrap_or_default()
    // }

    pub fn set_form(&mut self, form: &str) {
        self.form = Some(Self::parse_pairs(form));
    }

    /// 解析 form body，支持数组参数
    fn parse_form(form: &str) -> HashMap<String, Vec<String>> {
        Self::parse_pairs(form)
    }
}

#[cfg(test)]
mod tests {
    use super::*;


    #[test]
    fn test_parse_form_single() {
        let mut params = Params::new("/submit".to_string(), "/submit".to_string());
        let body = "name=alice&age=20";
        params.set_form(body);

        let form = params.form.as_ref().unwrap();
        assert_eq!(form.get("name").unwrap(), &vec!["alice".to_string()]);
        assert_eq!(form.get("age").unwrap(), &vec!["20".to_string()]);
    }

    #[test]
    fn test_parse_form_array() {
        let mut params = Params::new("/submit".to_string(), "/submit".to_string());
        let body = "tag=rust&tag=tokio&tag=async";
        params.set_form(body);

        let form = params.form.as_ref().unwrap();
        assert_eq!(
            form.get("tag").unwrap(),
            &vec!["rust".to_string(), "tokio".to_string(), "async".to_string()]
        );
    }

    #[test]
    fn test_parse_form_special_chars() {
        let mut params = Params::new("/submit".to_string(), "/submit".to_string());
        let body = "name=Alice+Bob&city=New+York&desc=Rust%20lang";
        params.set_form(body);

        let form = params.form.as_ref().unwrap();
        assert_eq!(form.get("name").unwrap(), &vec!["Alice Bob".to_string()]);
        assert_eq!(form.get("city").unwrap(), &vec!["New York".to_string()]);
        assert_eq!(form.get("desc").unwrap(), &vec!["Rust lang".to_string()]);
    }

    #[test]
    fn test_parse_form_parse_form_static() {
        let body = "a=1&a=2&b=3";
        let map = Params::parse_form(body);
        assert_eq!(map.get("a").unwrap(), &vec!["1".to_string(), "2".to_string()]);
        assert_eq!(map.get("b").unwrap(), &vec!["3".to_string()]);
    }

    #[test]
    fn test_parse_pairs_empty() {
        let map = Params::parse_pairs("");
        assert!(map.is_empty());
    }

    #[test]
    fn test_parse_pairs_single() {
        let map = Params::parse_pairs("key=value");
        assert_eq!(map.get("key").unwrap(), &vec!["value".to_string()]);
    }

    #[test]
    fn test_parse_pairs_multiple_values() {
        let map = Params::parse_pairs("a=1&a=2&b=3");
        assert_eq!(map.get("a").unwrap(), &vec!["1".to_string(), "2".to_string()]);
        assert_eq!(map.get("b").unwrap(), &vec!["3".to_string()]);
    }

    #[test]
    fn test_parse_pairs_no_value() {
        let map = Params::parse_pairs("key=&empty");
        assert_eq!(map.get("key").unwrap(), &vec!["".to_string()]);
        assert_eq!(map.get("empty").unwrap(), &vec!["".to_string()]);
    }

    #[test]
    fn test_parse_pairs_special_chars() {
        let map = Params::parse_pairs("name=Alice+Bob&city=New%20York&desc=Rust%26Tokio");
        assert_eq!(map.get("name").unwrap(), &vec!["Alice Bob".to_string()]);
        assert_eq!(map.get("city").unwrap(), &vec!["New York".to_string()]);
        assert_eq!(map.get("desc").unwrap(), &vec!["Rust&Tokio".to_string()]);
    }
}
