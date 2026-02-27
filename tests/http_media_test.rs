#[cfg(test)]
mod tests {
    use aex::http::protocol::media_type::{MediaType, SubMediaType};

    
    use std::path::Path;

    #[test]
    fn test_as_str() {
        assert_eq!(MediaType::Text.as_str(), "text");
        assert_eq!(MediaType::Image.as_str(), "image");
        assert_eq!(MediaType::Audio.as_str(), "audio");
        assert_eq!(MediaType::Video.as_str(), "video");
        assert_eq!(MediaType::Application.as_str(), "application");
        assert_eq!(MediaType::Multipart.as_str(), "multipart");
        assert_eq!(MediaType::Message.as_str(), "message");
        assert_eq!(MediaType::Font.as_str(), "font");
        assert_eq!(MediaType::Model.as_str(), "model");
        assert_eq!(MediaType::Unknown.as_str(), "unknown");
    }

    #[test]
    fn test_from_str_exact() {
        let all_pairs = [
            ("text", MediaType::Text),
            ("image", MediaType::Image),
            ("audio", MediaType::Audio),
            ("video", MediaType::Video),
            ("application", MediaType::Application),
            ("multipart", MediaType::Multipart),
            ("message", MediaType::Message),
            ("font", MediaType::Font),
            ("model", MediaType::Model),
            ("unknown", MediaType::Unknown),
        ];

        for (s, ty) in all_pairs.iter() {
            assert_eq!(MediaType::from_str(s), *ty);
            // 大小写不敏感
            assert_eq!(MediaType::from_str(&s.to_uppercase()), *ty);
            assert_eq!(MediaType::from_str(&s.to_ascii_lowercase()), *ty);
        }

        // 未知类型
        assert_eq!(MediaType::from_str("foobar"), MediaType::Unknown);
        assert_eq!(MediaType::from_str(""), MediaType::Unknown);
    }

    #[test]
    fn test_fromstr_trait() {
        let ty: MediaType = "text".parse().unwrap();
        assert_eq!(ty, MediaType::Text);

        let ty: MediaType = "IMAGE".parse().unwrap();
        assert_eq!(ty, MediaType::Image);

        let ty: MediaType = "unknown_type".parse().unwrap();
        assert_eq!(ty, MediaType::Unknown);
    }

    #[test]
    fn test_guess() {
        let cases = [
            ("index.html", "text/html"),
            ("style.htm", "text/html"),
            ("main.css", "text/css"),
            ("app.js", "application/javascript"),
            ("data.json", "application/json"),
            ("logo.png", "image/png"),
            ("photo.jpg", "image/jpeg"),
            ("photo.jpeg", "image/jpeg"),
            ("anim.gif", "image/gif"),
            ("readme.txt", "text/plain"),
            ("icon.svg", "image/svg+xml"),
            ("favicon.ico", "image/x-icon"),
            ("file.unknownext", "application/octet-stream"),
            ("noextension", "application/octet-stream"),
        ];

        for (filename, expected) in cases.iter() {
            let path = Path::new(filename);
            assert_eq!(MediaType::guess(path), *expected);
        }
    }

    // --- 1. MediaType 测试 ---

    #[test]
    fn test_media_type_as_str_and_from_str() {
        let types = [
            (MediaType::Text, "text"),
            (MediaType::Image, "image"),
            (MediaType::Audio, "audio"),
            (MediaType::Video, "video"),
            (MediaType::Application, "application"),
            (MediaType::Multipart, "multipart"),
            (MediaType::Message, "message"),
            (MediaType::Font, "font"),
            (MediaType::Model, "model"),
            (MediaType::Unknown, "unknown"),
        ];

        for (mt, s) in types {
            // 测试 as_str
            assert_eq!(mt.as_str(), s);
            // 测试 from_str (不区分大小写)
            assert_eq!(MediaType::from_str(&s.to_uppercase()), mt);
            // 测试 FromStr trait
            assert_eq!(MediaType::from_str(s), mt);
        }

        // 测试 Unknown 分支
        assert_eq!(MediaType::from_str("something/random"), MediaType::Unknown);
    }

    #[test]
    fn test_media_type_helpers() {
        assert!(MediaType::Application.is_application());
        assert!(MediaType::Text.is_text());
        assert!(MediaType::Multipart.is_multipart());
        assert!(MediaType::Audio.is_type(MediaType::Audio));

        // 覆盖反向判断（用于提高判定覆盖）
        assert!(!MediaType::Text.is_application());
        assert!(!MediaType::Image.is_text());
        assert!(!MediaType::Video.is_multipart());
    }

    #[test]
    fn test_media_type_guess() {
        let cases = [
            ("index.html", "text/html"),
            ("style.css", "text/css"),
            ("app.js", "application/javascript"),
            ("data.json", "application/json"),
            ("image.png", "image/png"),
            ("photo.jpg", "image/jpeg"),
            ("photo.jpeg", "image/jpeg"),
            ("anim.gif", "image/gif"),
            ("readme.txt", "text/plain"),
            ("logo.svg", "image/svg+xml"),
            ("favicon.ico", "image/x-icon"),
            ("file.unknown", "application/octet-stream"),
            ("no_extension", "application/octet-stream"),
        ];

        for (file, expected) in cases {
            assert_eq!(MediaType::guess(Path::new(file)), expected);
        }
    }

    // --- 2. SubMediaType 测试 ---

    #[test]
    fn test_sub_media_type_as_str() {
        // 穷举 match 的每一个分支以达到 100%
        let cases = [
            (SubMediaType::Json, "json"),
            (SubMediaType::UrlEncoded, "x-www-form-urlencoded"),
            (SubMediaType::OctetStream, "octet-stream"),
            (SubMediaType::Xml, "xml"),
            (SubMediaType::Pdf, "pdf"),
            (SubMediaType::Zip, "zip"),
            (SubMediaType::Javascript, "javascript"),
            (SubMediaType::FormData, "form-data"),
            (SubMediaType::Mixed, "mixed"),
            (SubMediaType::Plain, "plain"),
            (SubMediaType::Html, "html"),
            (SubMediaType::Css, "css"),
            (SubMediaType::Csv, "csv"),
            (SubMediaType::Png, "png"),
            (SubMediaType::Jpeg, "jpeg"),
            (SubMediaType::Gif, "gif"),
            (SubMediaType::Webp, "webp"),
            (SubMediaType::Svg, "svg+xml"),
            (SubMediaType::Icon, "x-icon"),
            (SubMediaType::Wasm, "wasm"),
            (SubMediaType::Unknown, "unknown"),
        ];

        for (smt, s) in cases {
            assert_eq!(smt.as_str(), s);
        }
    }

    #[test]
    fn test_sub_media_type_from_str() {
        // 测试常规解析
        assert_eq!(SubMediaType::from_str("JSON"), SubMediaType::Json);
        assert_eq!(
            SubMediaType::from_str("x-javascript"),
            SubMediaType::Javascript
        );
        assert_eq!(SubMediaType::from_str("jpg"), SubMediaType::Jpeg);

        // 覆盖带参数的字符串处理 (split(';').next())
        assert_eq!(
            SubMediaType::from_str("text/plain; charset=utf-8"),
            SubMediaType::Plain
        );

        // 覆盖 Unknown 分支
        assert_eq!(
            SubMediaType::from_str("invalid/subtype"),
            SubMediaType::Unknown
        );

        // 额外覆盖 FromStr trait
        assert_eq!("wasm".parse::<SubMediaType>().unwrap(), SubMediaType::Wasm);
    }

    #[test]
    fn test_sub_media_type_top_level_mapping() {
        // 覆盖所有分类映射
        assert_eq!(SubMediaType::Json.top_level(), MediaType::Application);
        assert_eq!(SubMediaType::FormData.top_level(), MediaType::Multipart);
        assert_eq!(SubMediaType::Html.top_level(), MediaType::Text);
        assert_eq!(SubMediaType::Png.top_level(), MediaType::Image);
        assert_eq!(SubMediaType::Unknown.top_level(), MediaType::Unknown);
    }

    #[test]
    fn test_sub_media_type_boolean_helpers() {
        // 覆盖所有的 matches! 宏分支
        assert!(SubMediaType::UrlEncoded.is_url_encoded());
        assert!(SubMediaType::FormData.is_form_data());
        assert!(SubMediaType::Json.is_json());

        // Web Resource
        assert!(SubMediaType::Html.is_web_resource());
        assert!(SubMediaType::Css.is_web_resource());
        assert!(SubMediaType::Javascript.is_web_resource());
        assert!(!SubMediaType::Plain.is_web_resource());

        // Images
        assert!(SubMediaType::Png.is_image());
        assert!(SubMediaType::Jpeg.is_image());
        assert!(SubMediaType::Gif.is_image());
        assert!(SubMediaType::Webp.is_image());
        assert!(SubMediaType::Svg.is_image());
        assert!(!SubMediaType::Icon.is_image()); // 注意：代码逻辑中 Icon 不在 is_image 列表里

        assert!(SubMediaType::Mixed.is_type(SubMediaType::Mixed));
    }

    #[test]
    fn test_media_type_exhaustive() {
        let all_variants = [
            (MediaType::Text, "text"),
            (MediaType::Image, "image"),
            (MediaType::Audio, "audio"),
            (MediaType::Video, "video"),
            (MediaType::Application, "application"),
            (MediaType::Multipart, "multipart"),
            (MediaType::Message, "message"),
            (MediaType::Font, "font"),
            (MediaType::Model, "model"),
            (MediaType::Unknown, "unknown"),
        ];

        for (variant, s) in all_variants {
            // 1. 覆盖 as_str 的每一个 match 分支
            assert_eq!(variant.as_str(), s);

            // 2. 覆盖 from_str 的每一个匹配分支 (包含大小写转换)
            let upper = s.to_uppercase();
            assert_eq!(MediaType::from_str(&upper), variant);

            // 3. 覆盖 FromStr trait 实现
            let parsed: MediaType = s.parse().unwrap();
            assert_eq!(parsed, variant);
        }

        // 4. 覆盖 from_str 的默认分支 (_)
        assert_eq!(MediaType::from_str("not_a_type"), MediaType::Unknown);
        assert_eq!(MediaType::from_str(""), MediaType::Unknown);
    }

    #[test]
    fn test_sub_media_type_exhaustive_as_str() {
        let all_variants = [
            (SubMediaType::Json, "json"),
            (SubMediaType::UrlEncoded, "x-www-form-urlencoded"),
            (SubMediaType::OctetStream, "octet-stream"),
            (SubMediaType::Xml, "xml"),
            (SubMediaType::Pdf, "pdf"),
            (SubMediaType::Zip, "zip"),
            (SubMediaType::Javascript, "javascript"),
            (SubMediaType::FormData, "form-data"),
            (SubMediaType::Mixed, "mixed"),
            (SubMediaType::Plain, "plain"),
            (SubMediaType::Html, "html"),
            (SubMediaType::Css, "css"),
            (SubMediaType::Csv, "csv"),
            (SubMediaType::Png, "png"),
            (SubMediaType::Jpeg, "jpeg"),
            (SubMediaType::Gif, "gif"),
            (SubMediaType::Webp, "webp"),
            (SubMediaType::Svg, "svg+xml"),
            (SubMediaType::Icon, "x-icon"),
            (SubMediaType::Wasm, "wasm"),
            (SubMediaType::Unknown, "unknown"),
        ];

        for (variant, s) in all_variants {
            // 覆盖 as_str 的所有分支
            assert_eq!(variant.as_str(), s);
        }
    }

    #[test]
    fn test_sub_media_type_exhaustive_from_str() {
        // 1. 覆盖 match 里的所有字符串字面量映射
        let cases = [
            ("json", SubMediaType::Json),
            ("x-www-form-urlencoded", SubMediaType::UrlEncoded),
            ("form-data", SubMediaType::FormData),
            ("octet-stream", SubMediaType::OctetStream),
            ("xml", SubMediaType::Xml),
            ("html", SubMediaType::Html),
            ("plain", SubMediaType::Plain),
            ("css", SubMediaType::Css),
            ("javascript", SubMediaType::Javascript),
            ("x-javascript", SubMediaType::Javascript), // 覆盖别名
            ("png", SubMediaType::Png),
            ("jpeg", SubMediaType::Jpeg),
            ("jpg", SubMediaType::Jpeg), // 覆盖别名
            ("gif", SubMediaType::Gif),
            ("webp", SubMediaType::Webp),
            ("svg+xml", SubMediaType::Svg),
            ("x-icon", SubMediaType::Icon),
            ("pdf", SubMediaType::Pdf),
            ("zip", SubMediaType::Zip),
            ("wasm", SubMediaType::Wasm),
            ("mixed", SubMediaType::Mixed),
            ("csv", SubMediaType::Csv),
        ];

        for (input, expected) in cases {
            assert_eq!(SubMediaType::from_str(input), expected);
        }

        // 2. 覆盖逻辑分支：带斜杠的全路径
        assert_eq!(SubMediaType::from_str("text/plain"), SubMediaType::Plain);

        // 3. 覆盖逻辑分支：带参数的字符串 (split(';'))
        assert_eq!(
            SubMediaType::from_str("application/json; charset=utf-8"),
            SubMediaType::Json
        );

        // 4. 覆盖逻辑分支：带空格和大小写 (trim(), to_lowercase())
        assert_eq!(SubMediaType::from_str("  IMAGE/PNG  "), SubMediaType::Png);

        // 5. 覆盖默认分支 (_)
        assert_eq!(
            SubMediaType::from_str("unknown_type"),
            SubMediaType::Unknown
        );
        assert_eq!(SubMediaType::from_str(""), SubMediaType::Unknown);
    }

    #[test]
    fn test_media_type_guess_all_extensions() {
        let cases = [
            ("t.html", "text/html"),
            ("t.htm", "text/html"),
            ("t.css", "text/css"),
            ("t.js", "application/javascript"),
            ("t.json", "application/json"),
            ("t.png", "image/png"),
            ("t.jpg", "image/jpeg"),
            ("t.jpeg", "image/jpeg"),
            ("t.gif", "image/gif"),
            ("t.txt", "text/plain"),
            ("t.svg", "image/svg+xml"),
            ("t.ico", "image/x-icon"),
            ("t.other", "application/octet-stream"), // 覆盖默认路径
            ("no_ext", "application/octet-stream"),  // 覆盖 None 路径
        ];

        for (path, expected) in cases {
            assert_eq!(MediaType::guess(Path::new(path)), expected);
        }
    }

    #[test]
    fn test_sub_media_type_boolean_logic_exhaustive() {
        // 1. is_url_encoded
        assert!(SubMediaType::UrlEncoded.is_url_encoded());
        assert!(!SubMediaType::Json.is_url_encoded()); // 触发 false 分支

        // 2. is_form_data
        assert!(SubMediaType::FormData.is_form_data());
        assert!(!SubMediaType::Mixed.is_form_data()); // 触发 false 分支

        // 3. is_json
        assert!(SubMediaType::Json.is_json());
        assert!(!SubMediaType::Xml.is_json()); // 触发 false 分支

        // 4. is_web_resource (包含三个匹配项的 || 逻辑)
        assert!(SubMediaType::Html.is_web_resource());
        assert!(SubMediaType::Css.is_web_resource());
        assert!(SubMediaType::Javascript.is_web_resource());
        assert!(!SubMediaType::Plain.is_web_resource()); // 触发全部不匹配的分支

        // 5. is_image (包含多个匹配项)
        assert!(SubMediaType::Png.is_image());
        assert!(SubMediaType::Jpeg.is_image());
        assert!(SubMediaType::Gif.is_image());
        assert!(SubMediaType::Webp.is_image());
        assert!(SubMediaType::Svg.is_image());
        assert!(!SubMediaType::Icon.is_image()); // 明确 Icon 为 false，覆盖 matches! 的默认穷举
    }

    #[test]
    fn test_sub_media_type_top_level_exhaustive() {
        let mapping = [
            (SubMediaType::Json, MediaType::Application),
            (SubMediaType::UrlEncoded, MediaType::Application),
            (SubMediaType::OctetStream, MediaType::Application),
            (SubMediaType::Xml, MediaType::Application),
            (SubMediaType::Pdf, MediaType::Application),
            (SubMediaType::Zip, MediaType::Application),
            (SubMediaType::Javascript, MediaType::Application),
            (SubMediaType::Wasm, MediaType::Application),
            (SubMediaType::FormData, MediaType::Multipart),
            (SubMediaType::Mixed, MediaType::Multipart),
            (SubMediaType::Plain, MediaType::Text),
            (SubMediaType::Html, MediaType::Text),
            (SubMediaType::Css, MediaType::Text),
            (SubMediaType::Csv, MediaType::Text),
            (SubMediaType::Png, MediaType::Image),
            (SubMediaType::Jpeg, MediaType::Image),
            (SubMediaType::Gif, MediaType::Image),
            (SubMediaType::Webp, MediaType::Image),
            (SubMediaType::Svg, MediaType::Image),
            (SubMediaType::Icon, MediaType::Image),
            (SubMediaType::Unknown, MediaType::Unknown),
        ];

        for (sub, top) in mapping {
            assert_eq!(sub.top_level(), top, "Mapping failed for {:?}", sub);
        }
    }

    #[test]
    fn test_media_type_boolean_logic_exhaustive() {
        // is_application
        assert!(MediaType::Application.is_application());
        assert!(!MediaType::Text.is_application());

        // is_text
        assert!(MediaType::Text.is_text());
        assert!(!MediaType::Video.is_text());

        // is_multipart
        assert!(MediaType::Multipart.is_multipart());
        assert!(!MediaType::Image.is_multipart());
    }
}
