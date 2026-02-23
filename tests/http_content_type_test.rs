
#[cfg(test)]
mod tests {
    use aex::http::protocol::{content_type::ContentType, media_type::{MediaType, SubMediaType}};


    #[test]
    fn test_top_level_parse() {
        assert_eq!(MediaType::from_str("text"), MediaType::Text);
        assert_eq!(MediaType::from_str("IMAGE"), MediaType::Image);
        assert_eq!(MediaType::from_str("unknown-type"), MediaType::Unknown);
    }

    #[test]
    fn test_content_type_parse() {
        let ct = ContentType::parse("text/html; charset=UTF-8");
        assert_eq!(ct.top_level, MediaType::Text);
        assert_eq!(ct.sub_type, SubMediaType::Html);
        assert_eq!(ct.parameters.len(), 1);
        assert_eq!(ct.parameters[0], ("charset".to_string(), "UTF-8".to_string()));

        let ct2 = ContentType::parse("application/json");
        assert_eq!(ct2.top_level, MediaType::Application);
        assert_eq!(ct2.sub_type, SubMediaType::Json);
        assert!(ct2.parameters.is_empty());
    }

    #[test]
    fn test_content_type_to_string() {
        let ct = ContentType::parse("text/html; charset=UTF-8");
        assert_eq!(ct.to_string(), "text/html; charset=UTF-8");
    }
}
