macro_rules! map {
    ({$($json:tt)+}) => {
        sentry::protocol::Map::from_iter(match serde_json::json!({$($json)+}) {
            serde_json::Value::Object(obj) => obj.into_iter(),
            _ => unreachable!(),
        })
    };
}

pub(crate) use map;
