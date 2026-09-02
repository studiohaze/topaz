use super::*;
use topaz_check::unit::{
    ExportedEnum, ExportedEnumVariant, ExportedNewtype, ExportedNominals, ExportedRecord,
    ExportedRecordField,
};
use topaz_check::{Ctor, Prim, Type};

fn contains(ty: &Type, nominals: &ExportedNominals) -> bool {
    web_type_contains_byte_buffer(ty, nominals, &mut std::collections::BTreeSet::new())
}

#[test]
fn web_abi_finds_byte_buffer_through_every_nominal_shape() {
    let mut nominals = ExportedNominals::default();
    nominals.records.insert(
        "Packet".to_string(),
        ExportedRecord {
            id: "Packet".to_string(),
            params: 0,
            fields: vec![ExportedRecordField {
                name: "body".to_string(),
                ty: Type::ByteBuffer,
                has_default: false,
            }],
            nominals: ExportedNominals::default(),
        },
    );
    nominals.enums.insert(
        "Message".to_string(),
        ExportedEnum {
            id: "Message".to_string(),
            params: 0,
            variants: vec![ExportedEnumVariant {
                name: "Data".to_string(),
                payloads: vec![Type::NominalRecord {
                    base: "Packet".to_string(),
                    args: vec![],
                }],
            }],
            nominals: ExportedNominals::default(),
        },
    );
    nominals.newtypes.insert(
        "Envelope".to_string(),
        ExportedNewtype {
            id: "Envelope".to_string(),
            params: 0,
            base: Type::Enum {
                base: "Message".to_string(),
                args: vec![],
            },
            nominals: ExportedNominals::default(),
        },
    );

    assert!(contains(
        &Type::Newtype {
            base: "Envelope".to_string(),
            args: vec![],
        },
        &nominals,
    ));
    assert!(contains(
        &Type::Ctor(Ctor::Option, vec![Type::ByteBuffer]),
        &nominals,
    ));
}

#[test]
fn web_abi_nominal_lookup_is_exact_and_cycle_safe() {
    let mut nominals = ExportedNominals::default();
    nominals.records.insert(
        "A.Node".to_string(),
        ExportedRecord {
            id: "A.Node".to_string(),
            params: 0,
            fields: vec![ExportedRecordField {
                name: "next".to_string(),
                ty: Type::NominalRecord {
                    base: "A.Node".to_string(),
                    args: vec![],
                },
                has_default: false,
            }],
            nominals: ExportedNominals::default(),
        },
    );
    nominals.records.insert(
        "B.Node".to_string(),
        ExportedRecord {
            id: "B.Node".to_string(),
            params: 0,
            fields: vec![ExportedRecordField {
                name: "body".to_string(),
                ty: Type::ByteBuffer,
                has_default: false,
            }],
            nominals: ExportedNominals::default(),
        },
    );

    assert!(!contains(
        &Type::NominalRecord {
            base: "A.Node".to_string(),
            args: vec![],
        },
        &nominals,
    ));
    assert!(contains(
        &Type::NominalRecord {
            base: "B.Node".to_string(),
            args: vec![],
        },
        &nominals,
    ));
    assert!(!contains(&Type::Prim(Prim::Int), &nominals));
}
