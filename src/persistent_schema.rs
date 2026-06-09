use std::collections::HashMap;

use crate::ast::SchemaType;
use crate::error::MarretaError;
use crate::route_loader::SchemaDefinition;

pub fn collect_persistent_schemas(
    schemas: &HashMap<String, SchemaDefinition>,
) -> HashMap<String, SchemaDefinition> {
    schemas
        .iter()
        .filter(|(_, schema)| schema.db_table.is_some())
        .map(|(name, schema)| (name.clone(), schema.clone()))
        .collect()
}

pub fn validate_persistent_schema_references(
    schemas: &HashMap<String, SchemaDefinition>,
) -> Result<(), MarretaError> {
    for (schema_name, schema) in schemas {
        if schema.db_table.is_none() {
            continue;
        }

        let id_fields: Vec<_> = schema
            .fields
            .iter()
            .filter(|field| field.name == "id")
            .collect();
        let id_field = match id_fields.as_slice() {
            [field] => *field,
            [] => {
                return Err(MarretaError::InvalidPersistentSchemaDefinition {
                    schema_name: schema_name.clone(),
                    message: "persistent schema must declare `id: integer`".to_string(),
                });
            }
            _ => {
                return Err(MarretaError::InvalidPersistentSchemaDefinition {
                    schema_name: schema_name.clone(),
                    message: "persistent schema declares `id` more than once".to_string(),
                });
            }
        };

        if id_field.optional {
            return Err(MarretaError::InvalidPersistentSchemaDefinition {
                schema_name: schema_name.clone(),
                message: "persistent schema `id` must not be optional".to_string(),
            });
        }

        if !matches!(id_field.field_type, SchemaType::IntegerType) {
            return Err(MarretaError::InvalidPersistentSchemaDefinition {
                schema_name: schema_name.clone(),
                message: "persistent schema `id` must be typed as integer".to_string(),
            });
        }

        for field in &schema.fields {
            if let SchemaType::Reference(target_schema) = &field.field_type
                && let Some(target) = schemas.get(target_schema)
                && target.db_table.is_none()
            {
                return Err(MarretaError::InvalidPersistentSchemaReference {
                    schema_name: schema_name.clone(),
                    field_name: field.name.clone(),
                    target_schema: target_schema.clone(),
                });
            }

            if let SchemaType::TypedList(inner) = &field.field_type
                && let SchemaType::Reference(target_schema) = inner.as_ref()
                && let Some(target) = schemas.get(target_schema)
                && target.db_table.is_some()
            {
                let inverse_fields: Vec<_> = target
                    .fields
                    .iter()
                    .filter(|target_field| {
                        matches!(
                            &target_field.field_type,
                            SchemaType::Reference(inverse_target) if inverse_target == schema_name
                        )
                    })
                    .collect();

                match inverse_fields.len() {
                    1 => {}
                    0 => {
                        return Err(MarretaError::InvalidPersistentSchemaDefinition {
                            schema_name: schema_name.clone(),
                            message: format!(
                                "list relation '{}' cannot be inferred because target schema '{}' does not reference '{}'",
                                field.name, target_schema, schema_name
                            ),
                        });
                    }
                    _ => {
                        let inverse_names = inverse_fields
                            .into_iter()
                            .map(|inverse_field| inverse_field.name.clone())
                            .collect::<Vec<_>>()
                            .join(", ");
                        return Err(MarretaError::InvalidPersistentSchemaDefinition {
                            schema_name: schema_name.clone(),
                            message: format!(
                                "list relation '{}' is ambiguous because target schema '{}' references '{}' through multiple fields: {}",
                                field.name, target_schema, schema_name, inverse_names
                            ),
                        });
                    }
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{SchemaField, SchemaType};

    fn field(name: &str, field_type: SchemaType) -> SchemaField {
        SchemaField {
            name: name.into(),
            field_type,
            optional: false,
        }
    }

    fn schema(db_table: Option<&str>, fields: Vec<SchemaField>) -> SchemaDefinition {
        SchemaDefinition {
            db_table: db_table.map(str::to_string),
            fields,
        }
    }

    #[test]
    fn test_collect_persistent_schemas_filters_by_db_table() {
        let mut schemas = HashMap::new();
        schemas.insert("User".into(), schema(Some("users"), vec![]));
        schemas.insert("Payload".into(), schema(None, vec![]));

        let persistent = collect_persistent_schemas(&schemas);
        assert_eq!(persistent.len(), 1);
        assert!(persistent.contains_key("User"));
    }

    #[test]
    fn test_validate_persistent_schema_allows_reference_to_persistent_schema() {
        let mut schemas = HashMap::new();
        schemas.insert(
            "Address".into(),
            schema(
                Some("addresses"),
                vec![field("id", SchemaType::IntegerType)],
            ),
        );
        schemas.insert(
            "User".into(),
            schema(
                Some("users"),
                vec![
                    field("id", SchemaType::IntegerType),
                    field("address", SchemaType::Reference("Address".into())),
                ],
            ),
        );

        assert!(validate_persistent_schema_references(&schemas).is_ok());
    }

    #[test]
    fn test_validate_persistent_schema_rejects_reference_to_contract_schema() {
        let mut schemas = HashMap::new();
        schemas.insert("AddressPayload".into(), schema(None, vec![]));
        schemas.insert(
            "User".into(),
            schema(
                Some("users"),
                vec![
                    field("id", SchemaType::IntegerType),
                    field("address", SchemaType::Reference("AddressPayload".into())),
                ],
            ),
        );

        let err = validate_persistent_schema_references(&schemas).unwrap_err();
        assert!(matches!(
            err,
            MarretaError::InvalidPersistentSchemaReference { .. }
        ));
    }

    #[test]
    fn test_validate_persistent_schema_requires_id_integer() {
        let mut schemas = HashMap::new();
        schemas.insert(
            "User".into(),
            schema(Some("users"), vec![field("name", SchemaType::StringType)]),
        );

        let err = validate_persistent_schema_references(&schemas).unwrap_err();
        assert!(matches!(
            err,
            MarretaError::InvalidPersistentSchemaDefinition { .. }
        ));
    }

    #[test]
    fn test_validate_persistent_schema_rejects_optional_id() {
        let mut schemas = HashMap::new();
        schemas.insert(
            "User".into(),
            SchemaDefinition {
                db_table: Some("users".into()),
                fields: vec![SchemaField {
                    name: "id".into(),
                    field_type: SchemaType::IntegerType,
                    optional: true,
                }],
            },
        );

        let err = validate_persistent_schema_references(&schemas).unwrap_err();
        assert!(matches!(
            err,
            MarretaError::InvalidPersistentSchemaDefinition { .. }
        ));
    }

    #[test]
    fn test_validate_persistent_schema_rejects_list_without_inverse_reference() {
        let mut schemas = HashMap::new();
        schemas.insert(
            "Order".into(),
            schema(Some("orders"), vec![field("id", SchemaType::IntegerType)]),
        );
        schemas.insert(
            "User".into(),
            SchemaDefinition {
                db_table: Some("users".into()),
                fields: vec![
                    SchemaField {
                        name: "id".into(),
                        field_type: SchemaType::IntegerType,
                        optional: false,
                    },
                    SchemaField {
                        name: "orders".into(),
                        field_type: SchemaType::TypedList(Box::new(SchemaType::Reference(
                            "Order".into(),
                        ))),
                        optional: false,
                    },
                ],
            },
        );

        let err = validate_persistent_schema_references(&schemas).unwrap_err();
        assert!(matches!(
            err,
            MarretaError::InvalidPersistentSchemaDefinition { .. }
        ));
    }

    #[test]
    fn test_validate_persistent_schema_rejects_ambiguous_list_inverse() {
        let mut schemas = HashMap::new();
        schemas.insert(
            "Order".into(),
            SchemaDefinition {
                db_table: Some("orders".into()),
                fields: vec![
                    SchemaField {
                        name: "id".into(),
                        field_type: SchemaType::IntegerType,
                        optional: false,
                    },
                    SchemaField {
                        name: "customer".into(),
                        field_type: SchemaType::Reference("User".into()),
                        optional: false,
                    },
                    SchemaField {
                        name: "billed_by".into(),
                        field_type: SchemaType::Reference("User".into()),
                        optional: false,
                    },
                ],
            },
        );
        schemas.insert(
            "User".into(),
            SchemaDefinition {
                db_table: Some("users".into()),
                fields: vec![
                    SchemaField {
                        name: "id".into(),
                        field_type: SchemaType::IntegerType,
                        optional: false,
                    },
                    SchemaField {
                        name: "orders".into(),
                        field_type: SchemaType::TypedList(Box::new(SchemaType::Reference(
                            "Order".into(),
                        ))),
                        optional: false,
                    },
                ],
            },
        );

        let err = validate_persistent_schema_references(&schemas).unwrap_err();
        assert!(matches!(
            err,
            MarretaError::InvalidPersistentSchemaDefinition { .. }
        ));
    }
}
