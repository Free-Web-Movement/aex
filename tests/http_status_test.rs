#[cfg(test)]
mod tests {
    use aex::http::protocol::status::StatusCode;

    #[test]
    fn test_from_u16_valid() {
        // 遍历所有有效状态码
        let codes = [
            100, 101, 102, 103, 200, 201, 202, 203, 204, 205, 206, 207, 208, 226, 300, 301, 302,
            303, 304, 305, 307, 308, 400, 401, 402, 403, 404, 405, 406, 407, 408, 409, 410, 411,
            412, 413, 414, 415, 416, 417, 418, 421, 422, 423, 424, 425, 426, 428, 429, 431, 451,
            500, 501, 502, 503, 504, 505, 506, 507, 508, 510, 511,
        ];

        for &code in codes.iter() {
            let status = StatusCode::from_u16(code);
            assert!(status.is_some(), "Code {} should be valid", code);
        }
    }

    #[test]
    fn test_from_u16_invalid() {
        // 测试无效 code 返回 None
        assert_eq!(StatusCode::from_u16(99), None);
        assert_eq!(StatusCode::from_u16(109), None);
        assert_eq!(StatusCode::from_u16(600), None);
    }

    #[test]
    fn test_to_str() {
        let all_statuses = [
            StatusCode::Continue,
            StatusCode::SwitchingProtocols,
            StatusCode::Processing,
            StatusCode::EarlyHints,
            StatusCode::Ok,
            StatusCode::Created,
            StatusCode::Accepted,
            StatusCode::NonAuthoritativeInformation,
            StatusCode::NoContent,
            StatusCode::ResetContent,
            StatusCode::PartialContent,
            StatusCode::MultiStatus,
            StatusCode::AlreadyReported,
            StatusCode::ImUsed,
            StatusCode::MultipleChoices,
            StatusCode::MovedPermanently,
            StatusCode::Found,
            StatusCode::SeeOther,
            StatusCode::NotModified,
            StatusCode::UseProxy,
            StatusCode::TemporaryRedirect,
            StatusCode::PermanentRedirect,
            StatusCode::BadRequest,
            StatusCode::Unauthorized,
            StatusCode::PaymentRequired,
            StatusCode::Forbidden,
            StatusCode::NotFound,
            StatusCode::MethodNotAllowed,
            StatusCode::NotAcceptable,
            StatusCode::ProxyAuthenticationRequired,
            StatusCode::RequestTimeout,
            StatusCode::Conflict,
            StatusCode::Gone,
            StatusCode::LengthRequired,
            StatusCode::PreconditionFailed,
            StatusCode::PayloadTooLarge,
            StatusCode::URITooLong,
            StatusCode::UnsupportedMediaType,
            StatusCode::RangeNotSatisfiable,
            StatusCode::ExpectationFailed,
            StatusCode::ImATeapot,
            StatusCode::MisdirectedRequest,
            StatusCode::UnprocessableEntity,
            StatusCode::Locked,
            StatusCode::FailedDependency,
            StatusCode::TooEarly,
            StatusCode::UpgradeRequired,
            StatusCode::PreconditionRequired,
            StatusCode::TooManyRequests,
            StatusCode::RequestHeaderFieldsTooLarge,
            StatusCode::UnavailableForLegalReasons,
            StatusCode::InternalServerError,
            StatusCode::NotImplemented,
            StatusCode::BadGateway,
            StatusCode::ServiceUnavailable,
            StatusCode::GatewayTimeout,
            StatusCode::HTTPVersionNotSupported,
            StatusCode::VariantAlsoNegotiates,
            StatusCode::InsufficientStorage,
            StatusCode::LoopDetected,
            StatusCode::NotExtended,
            StatusCode::NetworkAuthenticationRequired,
        ];

        for status in all_statuses.iter() {
            let s = status.to_str();
            assert!(
                !s.is_empty(),
                "Status {:?} string should not be empty",
                status
            );
        }
    }

    #[test]
    fn test_roundtrip_from_u16_to_str() {
        // 测试 from_u16 -> to_str 的回环
        let codes = [
            100, 101, 102, 103, 200, 201, 202, 203, 204, 205, 206, 207, 208, 226, 300, 301, 302,
            303, 304, 305, 307, 308, 400, 401, 402, 403, 404, 405, 406, 407, 408, 409, 410, 411,
            412, 413, 414, 415, 416, 417, 418, 421, 422, 423, 424, 425, 426, 428, 429, 431, 451,
            500, 501, 502, 503, 504, 505, 506, 507, 508, 510, 511,
        ];

        for &code in codes.iter() {
            let status = StatusCode::from_u16(code).unwrap();
            // 验证 to_str 不为空
            assert!(!status.to_str().is_empty());
        }
    }

    #[test]
    fn test_to_http_status_all_mapped() {
        use http::StatusCode as HttpStatus;
        let cases = [
            (StatusCode::Ok, HttpStatus::OK),
            (StatusCode::Created, HttpStatus::CREATED),
            (StatusCode::Accepted, HttpStatus::ACCEPTED),
            (StatusCode::NoContent, HttpStatus::NO_CONTENT),
            (StatusCode::BadRequest, HttpStatus::BAD_REQUEST),
            (StatusCode::Unauthorized, HttpStatus::UNAUTHORIZED),
            (StatusCode::Forbidden, HttpStatus::FORBIDDEN),
            (StatusCode::NotFound, HttpStatus::NOT_FOUND),
            (StatusCode::MethodNotAllowed, HttpStatus::METHOD_NOT_ALLOWED),
            (StatusCode::RequestTimeout, HttpStatus::REQUEST_TIMEOUT),
            (StatusCode::Conflict, HttpStatus::CONFLICT),
            (StatusCode::Gone, HttpStatus::GONE),
            (StatusCode::InternalServerError, HttpStatus::INTERNAL_SERVER_ERROR),
            (StatusCode::NotImplemented, HttpStatus::NOT_IMPLEMENTED),
            (StatusCode::BadGateway, HttpStatus::BAD_GATEWAY),
            (StatusCode::ServiceUnavailable, HttpStatus::SERVICE_UNAVAILABLE),
            (StatusCode::GatewayTimeout, HttpStatus::GATEWAY_TIMEOUT),
        ];
        for (aex_status, http_status) in cases.iter() {
            assert_eq!(aex_status.to_http_status(), *http_status);
        }
    }

    #[test]
    fn test_to_http_status_unmapped_falls_back_to_ok() {
        use http::StatusCode as HttpStatus;
        // 未显式映射的变体应回退到 OK
        let unmapped = [
            StatusCode::Continue,
            StatusCode::SwitchingProtocols,
            StatusCode::Processing,
            StatusCode::EarlyHints,
            StatusCode::NonAuthoritativeInformation,
            StatusCode::ResetContent,
            StatusCode::PartialContent,
            StatusCode::MultiStatus,
            StatusCode::AlreadyReported,
            StatusCode::ImUsed,
            StatusCode::MultipleChoices,
            StatusCode::MovedPermanently,
            StatusCode::Found,
            StatusCode::SeeOther,
            StatusCode::NotModified,
            StatusCode::UseProxy,
            StatusCode::TemporaryRedirect,
            StatusCode::PermanentRedirect,
            StatusCode::PaymentRequired,
            StatusCode::NotAcceptable,
            StatusCode::ProxyAuthenticationRequired,
            StatusCode::LengthRequired,
            StatusCode::PreconditionFailed,
            StatusCode::PayloadTooLarge,
            StatusCode::URITooLong,
            StatusCode::UnsupportedMediaType,
            StatusCode::RangeNotSatisfiable,
            StatusCode::ExpectationFailed,
            StatusCode::ImATeapot,
            StatusCode::MisdirectedRequest,
            StatusCode::UnprocessableEntity,
            StatusCode::Locked,
            StatusCode::FailedDependency,
            StatusCode::TooEarly,
            StatusCode::UpgradeRequired,
            StatusCode::PreconditionRequired,
            StatusCode::TooManyRequests,
            StatusCode::RequestHeaderFieldsTooLarge,
            StatusCode::UnavailableForLegalReasons,
            StatusCode::HTTPVersionNotSupported,
            StatusCode::VariantAlsoNegotiates,
            StatusCode::InsufficientStorage,
            StatusCode::LoopDetected,
            StatusCode::NotExtended,
            StatusCode::NetworkAuthenticationRequired,
        ];
        for status in unmapped.iter() {
            assert_eq!(status.to_http_status(), HttpStatus::OK);
        }
    }
}
