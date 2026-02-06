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
        let data = Self::extract_params(&url, &pattern);
        let query = Self::parse_query(&url);
        Self { url, data, query, pattern, form: None }
    }

    fn parse_pairs(pairs: &str) -> HashMap<String, Vec<String>> {
        let mut map: HashMap<String, Vec<String>> = HashMap::new();
        for (k, v) in form_urlencoded::parse(pairs.as_bytes()) {
            map.entry(k.into_owned()).or_default().push(v.into_owned());
        }
        map
    }

    /// 根据 URL 提取 query params
    /// 支持数组参数
    fn parse_query(url: &str) -> HashMap<String, Vec<String>> {
        url.split_once('?')
            .map(|(_, qs)| Self::parse_pairs(qs))
            .unwrap_or_default()
    }

    fn set_form(&mut self, form: &str) {
        self.form = Some(Self::parse_pairs(form));
    }

    /// 解析 form body，支持数组参数
    fn parse_form(form: &str) -> HashMap<String, Vec<String>> {
        Self::parse_pairs(form)
    }

    /// 将 path pattern 转为正则并提取变量名
    ///
    /// Examples:
    /// "/user/:id/profile" => regex: "/user/([^/]+)/profile", params: ["id"]
    /// "/file/:name.:ext"   => regex: "/file/([^/]+)\\.([^/]+)", params: ["name","ext"]
    /// "/static/*"          => regex: "/static/(.*)", params: ["*"]
    pub fn parse_path_regex(path: &str) -> (String, Vec<String>) {
        let mut regex_str = String::new();
        let mut param_names = Vec::new();
        let mut pos = 0;
        // let re = Regex::new(PATH_PARAMS).unwrap();

        for caps in PATH_PARAMS_RE.captures_iter(path) {
            let whole = caps.get(0).unwrap();
            let path_s = &path[pos..whole.start()];
            regex_str += &regex::escape(path_s);

            if let Some(_star) = caps.get(2) {
                // '*' 通配符
                regex_str += "(.*)";
                param_names.push("*".to_string());
            } else if let Some(name) = caps.get(1) {
                let name_str = name.as_str();
                if whole.as_str().ends_with('?') {
                    // 可选参数
                    // ⚠️ 修改点：捕获组外层加非捕获组包裹 /? 保证索引安全
                    regex_str += "(?:/([^/]+))?";
                } else {
                    regex_str += "([^/]+)";
                }
                param_names.push(name_str.to_string());
            }

            pos = whole.end();
        }

        // 剩余路径
        regex_str += &regex::escape(&path[pos..]);

        // ⚠️ 全匹配
        regex_str = format!("^{}$", regex_str);

        (regex_str, param_names)
    }

    /// 将 url 按正则 pattern 解析 path params
    pub fn extract_params(url: &str, pattern: &str) -> Option<HashMap<String, String>> {
        let (regex_str, param_names) = Self::parse_path_regex(pattern);

        let re = Regex::new(&regex_str).ok()?;
        let caps = re.captures(url)?;
        let mut map = HashMap::with_capacity(param_names.len());
        for (i, name) in param_names.iter().enumerate() {
            let value = caps.get(i + 1).map_or_else(String::new, |m| m.as_str().to_string());
            map.insert(name.clone(), value);
        }
        Some(map)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_path() {
        let url = "/user/123/profile";
        let pattern = "/user/:id/profile";
        let params = Params::extract_params(url, pattern).unwrap();
        assert_eq!(params.get("id").unwrap(), "123");
    }

    #[test]
    fn test_star_path() {
        let url = "/static/css/main.css";
        let pattern = "/static/*";
        let params = Params::extract_params(url, pattern).unwrap();
        assert_eq!(params.get("*").unwrap(), "css/main.css");
    }

    #[test]
    fn test_optional_param() {
        let url = "/user/";
        let pattern = "/user/:id?";
        let params = Params::extract_params(url, pattern).unwrap();
        assert_eq!(params.get("id").unwrap(), "");
    }

    #[test]
    #[should_panic(expected = "called `Option::unwrap()` on a `None` value")]
    fn test_optional_param_should_panic() {
        Params::extract_params("/user", "/user/:id?").unwrap();
    }

    #[test]
    fn test_ext_param() {
        let url = "/file/report.pdf";
        let pattern = "/file/:name.:ext";
        let params = Params::extract_params(url, pattern).unwrap();
        assert_eq!(params.get("name").unwrap(), "report");
        assert_eq!(params.get("ext").unwrap(), "pdf");
    }

    #[test]
    fn test_path_with_query() {
        let url = "/search?q=rust&sort=asc";
        let pattern = "/search";
        let params = Params::new(url.to_string(), pattern.to_string());
        assert!(params.data.is_none());
        assert_eq!(params.query.get("q").unwrap(), &vec!["rust".to_string()]);
        assert_eq!(params.query.get("sort").unwrap(), &vec!["asc".to_string()]);
    }

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
        assert_eq!(form.get("tag").unwrap(), &vec![
            "rust".to_string(),
            "tokio".to_string(),
            "async".to_string()
        ]);
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

    #[test]
    fn test_parse_query_empty() {
        let url = "/path";
        let map = Params::parse_query(url);
        assert!(map.is_empty());
    }

    #[test]
    fn test_parse_query_with_question_only() {
        let url = "/path?";
        let map = Params::parse_query(url);
        assert!(map.is_empty());
    }

    #[test]
    fn test_parse_query_normal() {
        let url = "/search?q=rust&sort=asc&sort=desc";
        let map = Params::parse_query(url);
        assert_eq!(map.get("q").unwrap(), &vec!["rust".to_string()]);
        assert_eq!(map.get("sort").unwrap(), &vec!["asc".to_string(), "desc".to_string()]);
    }

    #[test]
    fn test_parse_query_special_chars() {
        let url = "/search?q=Alice+Bob&city=New%20York";
        let map = Params::parse_query(url);
        assert_eq!(map.get("q").unwrap(), &vec!["Alice Bob".to_string()]);
        assert_eq!(map.get("city").unwrap(), &vec!["New York".to_string()]);
    }
    
}
