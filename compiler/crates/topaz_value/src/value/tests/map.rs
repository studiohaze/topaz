use super::*;

#[test]
fn indexed_map_keeps_order_and_distinct_key_kinds_through_mutation() {
    let mut map = OrderedMap::new();
    for (key, value) in [
        (Key::Int(1), 10),
        (Key::Str(Rc::from("1")), 20),
        (Key::Float(1.0), 30),
        (Key::Array(vec![Key::Int(1)]), 40),
        (Key::Str(Rc::from("last")), 50),
    ] {
        assert!(map.try_insert(key, Value::Int(value)));
    }
    map.insert(Key::Int(1), Value::Int(11));
    assert!(!map.try_insert(Key::Str(Rc::from("1")), Value::Int(999)));
    let values = |map: &OrderedMap| map.values().iter().map(render).collect::<Vec<_>>();
    assert_eq!(values(&map), ["11", "20", "30", "40", "50"]);
    assert!(matches!(map.remove(&Key::Float(1.0)), Some(Value::Int(30))));
    assert!(matches!(
        map.get(&Key::Str(Rc::from("last"))),
        Some(Value::Int(50))
    ));
    assert!(matches!(map.remove(&Key::Int(1)), Some(Value::Int(11))));
    map.insert(Key::Int(1), Value::Int(12));
    assert_eq!(values(&map), ["20", "40", "50", "12"]);
    assert!(matches!(
        map.get(&Key::Str(Rc::from("1"))),
        Some(Value::Int(20))
    ));
    assert!(matches!(
        map.get(&Key::Array(vec![Key::Int(1)])),
        Some(Value::Int(40))
    ));
    assert!(matches!(
        map.remove(&Key::Str(Rc::from("1"))),
        Some(Value::Int(20))
    ));
    assert!(map.get(&Key::Str(Rc::from("1"))).is_none());
    map.insert(Key::Str(Rc::from("1")), Value::Int(21));
    assert_eq!(values(&map), ["40", "50", "12", "21"]);
    map.clear();
    assert!(map.get(&Key::Int(1)).is_none());
    assert!(map.get(&Key::Str(Rc::from("last"))).is_none());
    assert!(map.try_insert(Key::Int(1), Value::Int(13)));
    assert_eq!(values(&map), ["13"]);
}

#[test]
fn indexed_map_preserves_nan_and_signed_zero_equality() {
    let mut map = OrderedMap::new();
    map.insert(Key::Float(f64::NAN), Value::Int(1));
    assert!(map.try_insert(Key::Float(f64::NAN), Value::Int(2)));
    assert!(map.get(&Key::Float(f64::NAN)).is_none());
    map.insert(Key::Float(-0.0), Value::Int(3));
    map.insert(Key::Float(0.0), Value::Int(4));
    map.insert(Key::Int(0), Value::Int(5));
    assert_eq!(map.len(), 4);
    assert!(matches!(map.get(&Key::Float(-0.0)), Some(Value::Int(4))));
    assert!(matches!(map.get(&Key::Int(0)), Some(Value::Int(5))));
}

#[test]
fn large_compiler_style_map_preserves_all_entries() {
    let mut map = OrderedMap::new();
    for index in 0..20_000 {
        map.insert(
            Key::Str(Rc::from(format!("node:{index}"))),
            Value::Int(index),
        );
    }
    for index in (0..20_000).rev() {
        assert!(
            matches!(map.get(&Key::Str(Rc::from(format!("node:{index}")))), Some(Value::Int(value)) if value == index)
        );
    }
    assert_eq!(map.len(), 20_000);
    assert!(matches!(map.values().first(), Some(Value::Int(0))));
    assert!(matches!(map.values().last(), Some(Value::Int(19_999))));
}
