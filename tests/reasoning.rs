use possums::catalog::{Catalog, Model, ReasoningOptions};
use serde_json::{json, Value};

fn model(params: Value) -> Model {
    Catalog::parse_authenticated(&serde_json::to_vec(&json!({"object":"list","data":[{
        "id":"synthetic","type":"chat","context_window":100,"endpoints":["/v1/chat/completions"],
        "pricing":{"inputTokenPricePer1M":1,"outputTokenPricePer1M":1,"requestPrice":0},
        "reasoning":true,"reasoning_params":params
    }]})).unwrap(), 0).unwrap().models.remove(0)
}
fn options(value: Value) -> ReasoningOptions {
    serde_json::from_value(value).unwrap()
}
fn mapped() -> Value {
    json!({"effort_map":{"low":"low","medium":"high","high":"xhigh"},"params":{
        "/v1/chat/completions":{"enable":{"chat_template_kwargs":{"thinking":true,"reasoning_effort":"$EFFORT"}},
        "disable":{"chat_template_kwargs":{"thinking":false}}},
        "/v1/responses":{"enable":{"unsupported_response_only":"ignored"}}
    }})
}
#[test]
fn catalog_templates_authorize_only_concrete_advertised_combinations() {
    let model = model(mapped());
    let metadata = model.reasoning_params.as_ref().unwrap();
    assert_eq!(metadata.params.len(), 1);
    assert_eq!(metadata.effort_map.as_ref().unwrap()["medium"], "high");
    assert!(model.allows_reasoning(&ReasoningOptions::default()));
    for effort in ["low", "high", "xhigh"] {
        assert!(model.allows_reasoning(&options(
            json!({"chat_template_kwargs":{"thinking":true,"reasoning_effort":effort}})
        )));
    }
    assert!(model.allows_reasoning(&options(json!({"chat_template_kwargs":{"thinking":false}}))));
    for value in [
        json!({"reasoning_effort":"high"}),
        json!({"chat_template_kwargs":{"thinking":false,"reasoning_effort":"high"}}),
        json!({"chat_template_kwargs":{"thinking":true,"reasoning_effort":"medium"}}),
        json!({"chat_template_kwargs":{"thinking":true,"reasoning_effort":"$EFFORT"}}),
        json!({"chat_template_kwargs":{"thinking":true,"private-marker":true}}),
        json!({"chat_template_kwargs":{}}),
    ] {
        assert!(!model.allows_reasoning(&options(value)));
    }
}
#[test]
fn unknown_catalog_shapes_leave_defaults_available_without_controls() {
    for value in [
        Value::Null,
        json!({}),
        json!({"effort_map":null,"params":{"/v1/chat/completions":{"enable":{"chat_template_kwargs":{"thinking":true}}}}}),
        json!({"params":{"/v1/chat/completions":{"enable":null,"disable":{"chat_template_kwargs":{"thinking":false}}}}}),
        json!({"params":{"/v1/chat/completions":{"enable":{"reasoning_effort":null,"chat_template_kwargs":{"thinking":true}}}}}),
        json!({"params":"private-marker"}),
        json!({"params":{"/v1/chat/completions":{"enable":{"temperature":1}}}}),
        json!({"params":{"/v1/chat/completions":{"enable":{"chat_template_kwargs":{"thinking":"private-marker"}}}}}),
        json!({"params":{"/v1/chat/completions":{"enable":{"chat_template_kwargs":{"thinking":true,"other":true}}}}}),
        json!({"params":{"/v1/chat/completions":{"disable":{"reasoning_effort":"$EFFORT"}}}}),
        json!({"effort_map":{"other":"high"},"params":{"/v1/chat/completions":{"enable":{"reasoning_effort":"$EFFORT"}}}}),
        json!({"effort_map":{"high":"private-marker"},"params":{"/v1/chat/completions":{"enable":{"reasoning_effort":"$EFFORT"}}}}),
    ] {
        let model = model(value);
        assert!(model.reasoning_params.is_none());
        assert!(model.allows_reasoning(&ReasoningOptions::default()));
        assert!(!model.allows_reasoning(&options(json!({"reasoning_effort":"high"}))));
    }
}
#[test]
fn unmapped_placeholder_is_not_an_effort_allowlist_but_fixed_enable_is_a_toggle() {
    let unmapped =
        model(json!({"params":{"/v1/chat/completions":{"enable":{"reasoning_effort":"$EFFORT"}}}}));
    assert!(unmapped.reasoning_params.is_some());
    for effort in ["low", "medium", "high", "max"] {
        assert!(!unmapped.allows_reasoning(&options(json!({"reasoning_effort":effort}))));
    }
    let toggle = model(
        json!({"params":{"/v1/chat/completions":{"enable":{"chat_template_kwargs":{"enable_thinking":true}}}}}),
    );
    assert!(toggle.allows_reasoning(&options(
        json!({"chat_template_kwargs":{"enable_thinking":true}})
    )));
    assert!(!toggle.allows_reasoning(&options(
        json!({"chat_template_kwargs":{"enable_thinking":false}})
    )));
}
#[test]
fn sdk_set_preserves_explicit_controls_and_omission() {
    let builder = || {
        tinfoil::relaxed::RelaxedChatRequestBuilder::new()
            .model("synthetic")
            .messages(vec![json!({"role":"user","content":"synthetic"})])
    };
    let plain = serde_json::to_value(ReasoningOptions::default().apply(builder()).build()).unwrap();
    assert!(plain.get("reasoning_effort").is_none());
    assert!(plain.get("chat_template_kwargs").is_none());
    let disabled = serde_json::to_value(
        options(json!({"chat_template_kwargs":{"thinking":false}}))
            .apply(builder())
            .build(),
    )
    .unwrap();
    assert_eq!(disabled["chat_template_kwargs"], json!({"thinking":false}));
    assert_eq!(disabled["messages"], plain["messages"]);
}
