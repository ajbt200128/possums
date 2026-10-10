//! Sole typed fail-fast observations; no response-status inference.
use super::*;
use http::request::Builder;

struct Case {
    request: Builder,
    body: &'static str,
    status: u16,
    endpoint: Endpoint,
    reason: Option<Rejection>,
    authenticated: bool,
}
impl Case {
    fn new(
        request: Builder,
        body: &'static str,
        status: u16,
        endpoint: Endpoint,
        reason: Option<Rejection>,
    ) -> Self {
        Self {
            request,
            body,
            status,
            endpoint,
            reason,
            authenticated: false,
        }
    }
    fn authenticated(mut self) -> Self {
        self.authenticated = true;
        self
    }
}

async fn assert_cases(api: bool, failure: Failure, cases: Vec<Case>) {
    for case in cases {
        let metrics = metrics(Deployment::Production);
        let f = Fixture::new(api, failure, Some(metrics.clone()));
        let mut request = case.request;
        if case.authenticated {
            request = request.header("authorization", format!("Bearer {}", f.credential));
        }
        let response = router(f.state.clone())
            .oneshot(request.body(Body::from(case.body)).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), case.status);
        drain(response.into_body()).await;
        f.quiescent().await;
        assert_eq!(totals(&metrics), (1, 1, 0, 0));
        assert_eq!(
            f.state.accounting.available("private-account-canary"),
            Some(100)
        );
        let state = metrics.state.lock().unwrap();
        let t = &state.requests.active;
        assert_eq!(
            t.rejected.iter().sum::<u64>(),
            u64::from(case.reason.is_some())
        );
        let disposition = if let Some(reason) = case.reason {
            let model = if case.endpoint.chat().is_some() {
                AdmissionModel::Unknown
            } else {
                AdmissionModel::NotApplicable
            };
            assert_eq!(t.rejected(Some(case.endpoint), model, reason), Some(1));
            Disposition::PreReservationRejected
        } else {
            Disposition::ControlOrOther
        };
        let status = match case.status / 100 {
            2 => Status::Success,
            4 => Status::ClientError,
            5 => Status::ServerError,
            _ => panic!("unexpected fixture status"),
        };
        assert_eq!(
            t.dispositions[http_index(case.endpoint, status, HttpTerminal::Eof)]
                [disposition as usize],
            1
        );
    }
}

#[tokio::test]
async fn api_control_rejections_keep_input_auth_and_catalog_boundaries() {
    use Endpoint::*;
    use Rejection::{Auth, Input};
    assert_cases(
        true,
        Failure::None,
        vec![
            Case::new(Request::get("/v1/models"), "", 401, Models, Some(Auth)),
            Case::new(
                Request::post("/v1/submissions"),
                "",
                401,
                ApiSubmission,
                Some(Auth),
            ),
            Case::new(
                Request::delete("/v1/sessions/current"),
                "",
                401,
                ApiLogout,
                Some(Auth),
            ),
            Case::new(
                Request::get("/v1/models"),
                "hostile-body-canary",
                400,
                Models,
                Some(Input),
            )
            .authenticated(),
            Case::new(
                Request::get("/v1/models").header("cookie", "hostile-cookie-canary"),
                "",
                400,
                Models,
                Some(Input),
            )
            .authenticated(),
            Case::new(
                Request::post("/v1/chat/completions").header("content-encoding", "gzip"),
                "",
                400,
                ChatApi,
                Some(Input),
            ),
            Case::new(
                Request::get("/v1/auth/challenge"),
                "",
                400,
                ApiChallenge,
                Some(Input),
            )
            .authenticated(),
            Case::new(
                Request::get("/v1/auth/challenge").header("content-type", "application/json"),
                "",
                400,
                ApiChallenge,
                Some(Input),
            ),
            Case::new(
                Request::post("/v1/sessions").header("content-type", "application/json"),
                "hostile-json-canary",
                400,
                ApiSession,
                Some(Input),
            ),
            Case::new(
                Request::post("/v1/sessions").header("content-type", "application/json"),
                r#"{"challenge":"invalid","credential":"hostile-credential-canary"}"#,
                400,
                ApiSession,
                Some(Input),
            ),
            Case::new(
                Request::post("/v1/submissions").header("content-type", "application/json"),
                r#"{"model":"","new_conversation":false}"#,
                400,
                ApiSubmission,
                Some(Input),
            )
            .authenticated(),
            Case::new(
                Request::post("/v1/submissions").header("content-type", "application/json"),
                r#"{"model":"absent","new_conversation":false}"#,
                400,
                ApiSubmission,
                Some(Input),
            )
            .authenticated(),
            Case::new(
                Request::delete("/v1/sessions/current").header("content-type", "application/json"),
                "",
                400,
                ApiLogout,
                Some(Input),
            )
            .authenticated(),
            Case::new(
                Request::get("/v1/models?hostile-query-canary"),
                "",
                400,
                Models,
                Some(Input),
            ),
            Case::new(Request::get("/v1/missing"), "", 404, Other, None),
        ],
    )
    .await;
    for (failure, reason) in [
        (Failure::Verification, Rejection::Verification),
        (Failure::Catalog, Rejection::Catalog),
    ] {
        assert_cases(
            true,
            failure,
            vec![
                Case::new(Request::get("/v1/models"), "", 503, Models, Some(reason))
                    .authenticated(),
                Case::new(
                    Request::post("/v1/submissions").header("content-type", "application/json"),
                    r#"{"model":"kimi-k3","new_conversation":false}"#,
                    503,
                    ApiSubmission,
                    Some(reason),
                )
                .authenticated(),
            ],
        )
        .await;
    }
}
