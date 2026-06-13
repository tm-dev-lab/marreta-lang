/// OpenAPI 3.0 spec builder for MarretaLang v0.3.3.
///
/// Generates a JSON object from the RouteRegistry at startup and holds it
/// in an `Arc<String>` for zero-copy serving on every request.
use std::collections::{HashMap, HashSet};

use serde_json::{Value as Json, json};

use crate::ast::{
    Argument, AuthProvider, Expression, HttpVerb, MapStatement, PipelineStage, ReplyContentType,
    RescueHandler, SchemaType, Statement, TakeBinding, TaskBody,
};
use crate::file_loader::ProjectRuntime;
use crate::route_loader::{ConsumerKind, RouteDefinition, RouteRegistry, SchemaDefinition};

/// Builds an OpenAPI 3.0 JSON string from the route registry.
pub fn build(registry: &RouteRegistry, title: &str, version: &str) -> String {
    build_inner(registry, None, title, version)
}

/// Builds an OpenAPI 3.0 JSON string using project runtime metadata.
///
/// The runtime is needed to resolve file-private schemas used by public routes.
/// Public routes expose those schemas as part of the external API contract even
/// when the schema is not exported across Marreta source files.
pub fn build_with_runtime(
    registry: &RouteRegistry,
    runtime: &ProjectRuntime,
    title: &str,
    version: &str,
) -> String {
    build_inner(registry, Some(runtime), title, version)
}

fn build_inner(
    registry: &RouteRegistry,
    runtime: Option<&ProjectRuntime>,
    title: &str,
    version: &str,
) -> String {
    let mut paths: HashMap<String, Json> = HashMap::new();
    let security_schemes = build_security_schemes(&registry.auth_providers);
    let components_schemas = build_component_schemas(registry, runtime);

    let component_names: HashSet<String> = components_schemas.keys().cloned().collect();

    // --- Build paths ---
    let mut tag_names: Vec<String> = Vec::new();
    let mut operation_ids: HashMap<String, usize> = HashMap::new();
    for route in &registry.routes {
        let openapi_path = to_openapi_path(&route.path);
        let verb = verb_str(&route.verb);

        let tag = route
            .source_file
            .as_deref()
            .map(stem_to_tag)
            .unwrap_or_else(|| title.to_string());

        if !tag_names.contains(&tag) {
            tag_names.push(tag.clone());
        }

        let mut operation = build_operation(route, &tag, &component_names);
        operation["operationId"] = json!(operation_id(route, &mut operation_ids));

        if let Some(request_body) = build_request_body(route, &component_names) {
            operation["requestBody"] = request_body;
        }
        if has_headers_binding(route) {
            operation["x-marreta-bindings"] = json!({ "headers": true });
        }

        let path_entry = paths.entry(openapi_path).or_insert_with(|| json!({}));
        path_entry[verb] = operation;
    }

    // If no routes had a source_file, fall back to a single tag from project_name/title
    if tag_names.is_empty() {
        tag_names.push(title.to_string());
    }
    let tags_json: Vec<Json> = tag_names.iter().map(|n| json!({ "name": n })).collect();

    let mut spec = json!({
        "openapi": "3.0.3",
        "info": {
            "title": title,
            "version": version
        },
        "tags": tags_json,
        "paths": paths
    });

    if !components_schemas.is_empty() || !security_schemes.is_empty() {
        let mut components = json!({});
        if !components_schemas.is_empty() {
            components["schemas"] = json!(components_schemas);
        }
        if !security_schemes.is_empty() {
            components["securitySchemes"] = json!(security_schemes);
        }
        spec["components"] = components;
    }

    // x-marreta-consumers — informational extension listing all queue/topic consumers
    if !registry.consumers.is_empty() {
        let consumers_json: Vec<Json> = registry
            .consumers
            .iter()
            .map(|c| {
                let target =
                    extract_string_literal(&c.target).unwrap_or_else(|| "<dynamic>".to_string());
                match c.kind {
                    ConsumerKind::Queue => {
                        let mut obj = json!({ "kind": "queue", "target": target });
                        if let Some(schema) = &c.schema {
                            obj["schema"] = json!(schema);
                        }
                        obj
                    }
                    ConsumerKind::Topic => {
                        let mut obj = json!({ "kind": "topic", "pattern": target });
                        if let Some(schema) = &c.schema {
                            obj["schema"] = json!(schema);
                        }
                        obj
                    }
                }
            })
            .collect();
        spec["x-marreta-consumers"] = json!(consumers_json);
    }

    serde_json::to_string_pretty(&spec).unwrap_or_else(|_| "{}".to_string())
}

fn build_component_schemas(
    registry: &RouteRegistry,
    runtime: Option<&ProjectRuntime>,
) -> HashMap<String, Json> {
    let mut resolved: HashMap<String, SchemaDefinition> = registry.schemas.clone();
    let mut pending = Vec::new();

    for route in &registry.routes {
        collect_route_schema_refs(route, &mut pending);
    }
    for schema in resolved.values() {
        collect_nested_schema_refs(schema, None, &mut pending);
    }

    while let Some((schema_name, module_id)) = pending.pop() {
        if resolved.contains_key(&schema_name) {
            continue;
        }
        if let Some(schema) =
            resolve_schema_for_docs(registry, runtime, module_id.as_deref(), &schema_name)
        {
            collect_nested_schema_refs(&schema, module_id.clone(), &mut pending);
            resolved.insert(schema_name, schema);
        }
    }

    resolved
        .into_iter()
        .map(|(name, schema)| (name, schema_definition_to_openapi(&schema)))
        .collect()
}

fn schema_definition_to_openapi(schema_def: &SchemaDefinition) -> Json {
    let mut properties: HashMap<String, Json> = HashMap::new();
    let mut required: Vec<String> = Vec::new();

    for field in &schema_def.fields {
        let prop = schema_type_to_openapi(&field.field_type);
        properties.insert(field.name.clone(), prop);
        if !field.optional {
            required.push(field.name.clone());
        }
    }

    let mut schema_obj = json!({
        "type": "object",
        "properties": properties
    });
    if !required.is_empty() {
        schema_obj["required"] = json!(required);
    }
    schema_obj
}

fn resolve_schema_for_docs(
    registry: &RouteRegistry,
    runtime: Option<&ProjectRuntime>,
    module_id: Option<&str>,
    schema_name: &str,
) -> Option<SchemaDefinition> {
    runtime
        .and_then(|runtime| runtime.resolve_schema(module_id, schema_name))
        .or_else(|| registry.schemas.get(schema_name).cloned())
}

fn collect_route_schema_refs(route: &RouteDefinition, out: &mut Vec<(String, Option<String>)>) {
    if has_payload_binding(route)
        && let Some(schema) = &route.schema
    {
        out.push((schema.clone(), route.module_id.clone()));
    }
    collect_response_schema_refs(&route.body, route.module_id.clone(), out);
}

fn collect_response_schema_refs(
    body: &[Statement],
    module_id: Option<String>,
    out: &mut Vec<(String, Option<String>)>,
) {
    for stmt in body {
        match stmt {
            Statement::Reply {
                response_schema,
                body,
                ..
            } => {
                if let Some(schema) = response_schema {
                    out.push((schema.clone(), module_id.clone()));
                }
                collect_expression_schema_refs(body, module_id.clone(), out);
            }
            Statement::Transaction { body, .. }
            | Statement::While { body, .. }
            | Statement::OnQueue { body, .. }
            | Statement::OnTopic { body, .. } => {
                collect_response_schema_refs(body, module_id.clone(), out);
            }
            Statement::TaskDef { body, .. } => {
                collect_task_body_schema_refs(body, module_id.clone(), out)
            }
            Statement::ExpressionStatement { expression, .. }
            | Statement::Assignment {
                value: expression, ..
            }
            | Statement::ConditionalAssignment {
                value: expression, ..
            }
            | Statement::Fail {
                message: expression,
                ..
            }
            | Statement::Raise {
                message: expression,
                ..
            } => collect_expression_schema_refs(expression, module_id.clone(), out),
            Statement::Export(inner) => collect_response_schema_refs(
                std::slice::from_ref(inner.as_ref()),
                module_id.clone(),
                out,
            ),
            _ => {}
        }
    }
}

fn collect_task_body_schema_refs(
    body: &TaskBody,
    module_id: Option<String>,
    out: &mut Vec<(String, Option<String>)>,
) {
    match body {
        TaskBody::Inline(expr) => collect_expression_schema_refs(expr, module_id, out),
        TaskBody::Block(statements, expr) => {
            collect_response_schema_refs(statements, module_id.clone(), out);
            collect_expression_schema_refs(expr, module_id, out);
        }
    }
}

fn collect_expression_schema_refs(
    expr: &Expression,
    module_id: Option<String>,
    out: &mut Vec<(String, Option<String>)>,
) {
    match expr {
        Expression::SchemaConstructor {
            schema_name,
            fields,
        } => {
            out.push((schema_name.clone(), module_id.clone()));
            for (_, value) in fields {
                collect_expression_schema_refs(value, module_id.clone(), out);
            }
        }
        Expression::HttpClientResponseSchema { call, schema_name } => {
            out.push((schema_name.clone(), module_id.clone()));
            collect_expression_schema_refs(call, module_id, out);
        }
        Expression::List(items) => {
            for item in items {
                collect_expression_schema_refs(item, module_id.clone(), out);
            }
        }
        Expression::MapLiteral(items) => {
            for (_, value) in items {
                collect_expression_schema_refs(value, module_id.clone(), out);
            }
        }
        Expression::BinaryOp { left, right, .. } => {
            collect_expression_schema_refs(left, module_id.clone(), out);
            collect_expression_schema_refs(right, module_id, out);
        }
        Expression::UnaryOp { operand, .. }
        | Expression::PropertyAccess {
            object: operand, ..
        } => {
            collect_expression_schema_refs(operand, module_id, out);
        }
        Expression::MethodCall {
            object, arguments, ..
        } => {
            collect_expression_schema_refs(object, module_id.clone(), out);
            collect_argument_schema_refs(arguments, module_id, out);
        }
        Expression::FunctionCall { arguments, .. } => {
            collect_argument_schema_refs(arguments, module_id, out);
        }
        Expression::Match { subject, arms } => {
            collect_expression_schema_refs(subject, module_id.clone(), out);
            for arm in arms {
                collect_expression_schema_refs(&arm.value, module_id.clone(), out);
            }
        }
        Expression::If {
            condition,
            then_branch,
            else_branch,
        } => {
            collect_expression_schema_refs(condition, module_id.clone(), out);
            collect_task_body_schema_refs(then_branch, module_id.clone(), out);
            if let Some(else_branch) = else_branch {
                collect_task_body_schema_refs(else_branch, module_id, out);
            }
        }
        Expression::Subscript { object, key } => {
            collect_expression_schema_refs(object, module_id.clone(), out);
            collect_expression_schema_refs(key, module_id, out);
        }
        Expression::Pipeline { input, stages } => {
            collect_expression_schema_refs(input, module_id.clone(), out);
            for stage in stages {
                collect_pipeline_stage_schema_refs(stage, module_id.clone(), out);
            }
        }
        Expression::Broadcast { input, targets } => {
            collect_expression_schema_refs(input, module_id.clone(), out);
            for target in targets {
                collect_expression_schema_refs(target, module_id.clone(), out);
            }
        }
        Expression::Rescue { expr, handler } => {
            collect_expression_schema_refs(expr, module_id.clone(), out);
            collect_expression_schema_refs(handler, module_id, out);
        }
        Expression::QueuePush {
            queue_name,
            schema,
            payload,
        } => {
            collect_expression_schema_refs(queue_name, module_id.clone(), out);
            if let Some(schema) = schema {
                out.push((schema.clone(), module_id.clone()));
            }
            if let Some(payload) = payload {
                collect_expression_schema_refs(payload, module_id, out);
            }
        }
        Expression::TopicPublish {
            topic,
            schema,
            payload,
        } => {
            collect_expression_schema_refs(topic, module_id.clone(), out);
            if let Some(schema) = schema {
                out.push((schema.clone(), module_id.clone()));
            }
            if let Some(payload) = payload {
                collect_expression_schema_refs(payload, module_id, out);
            }
        }
        _ => {}
    }
}

fn collect_argument_schema_refs(
    arguments: &[Argument],
    module_id: Option<String>,
    out: &mut Vec<(String, Option<String>)>,
) {
    for arg in arguments {
        match arg {
            Argument::Positional(expr) | Argument::Named { value: expr, .. } => {
                collect_expression_schema_refs(expr, module_id.clone(), out)
            }
        }
    }
}

fn collect_pipeline_stage_schema_refs(
    stage: &PipelineStage,
    module_id: Option<String>,
    out: &mut Vec<(String, Option<String>)>,
) {
    match stage {
        PipelineStage::Expression(expr) => collect_expression_schema_refs(expr, module_id, out),
        PipelineStage::Map { body, .. } => {
            for stmt in body {
                match stmt {
                    MapStatement::Statement(stmt) => collect_response_schema_refs(
                        std::slice::from_ref(stmt),
                        module_id.clone(),
                        out,
                    ),
                    MapStatement::Keep { value, condition } => {
                        collect_expression_schema_refs(value, module_id.clone(), out);
                        if let Some(condition) = condition {
                            collect_expression_schema_refs(condition, module_id.clone(), out);
                        }
                    }
                    MapStatement::Skip { condition } => {
                        collect_expression_schema_refs(condition, module_id.clone(), out);
                    }
                }
            }
        }
        PipelineStage::Reduce { initial, body, .. } => {
            collect_expression_schema_refs(initial, module_id.clone(), out);
            collect_task_body_schema_refs(body, module_id, out);
        }
        PipelineStage::Rescue { handler } => {
            collect_rescue_handler_schema_refs(handler, module_id, out)
        }
    }
}

fn collect_rescue_handler_schema_refs(
    handler: &RescueHandler,
    module_id: Option<String>,
    out: &mut Vec<(String, Option<String>)>,
) {
    match handler {
        RescueHandler::Inline(expr) => collect_expression_schema_refs(expr, module_id, out),
        RescueHandler::Block(statements) => {
            collect_response_schema_refs(statements, module_id, out)
        }
    }
}

fn collect_nested_schema_refs(
    schema: &SchemaDefinition,
    module_id: Option<String>,
    out: &mut Vec<(String, Option<String>)>,
) {
    for field in &schema.fields {
        collect_schema_type_refs(&field.field_type, module_id.clone(), out);
    }
}

fn collect_schema_type_refs(
    schema_type: &SchemaType,
    module_id: Option<String>,
    out: &mut Vec<(String, Option<String>)>,
) {
    match schema_type {
        SchemaType::Reference(name) => out.push((name.clone(), module_id)),
        SchemaType::TypedList(inner) => collect_schema_type_refs(inner, module_id, out),
        _ => {}
    }
}

fn build_operation(route: &RouteDefinition, tag: &str, component_names: &HashSet<String>) -> Json {
    let mut parameters: Vec<Json> = Vec::new();

    // URL path parameters — extract from `:param` segments
    for segment in route.path.split('/') {
        if let Some(param_name) = segment.strip_prefix(':') {
            parameters.push(json!({
                "name": param_name,
                "in": "path",
                "required": true,
                "schema": { "type": "string" }
            }));
        }
    }

    // Query parameter binding
    if has_query_binding(route) {
        parameters.push(json!({
            "name": "query",
            "in": "query",
            "required": false,
            "style": "deepObject",
            "explode": true,
            "schema": {
                "type": "object",
                "additionalProperties": { "type": "string" }
            },
            "description": "All query string parameters bound as a Marreta map"
        }));
    }

    // Build responses from the route body AST
    let mut responses: serde_json::Map<String, Json> = serde_json::Map::new();
    let outcomes = collect_route_outcomes(&route.body);
    for response in outcomes.responses {
        let response_obj = response_to_openapi(&response, component_names);
        match response.code {
            Some(code) => {
                responses.entry(code.to_string()).or_insert(response_obj);
            }
            None => {
                responses
                    .entry("default".to_string())
                    .or_insert(response_obj);
            }
        }
    }
    // Error responses (Fail/Require/Reject) — no body, just status
    for code in outcomes.error_codes {
        let code_str = code.to_string();
        responses
            .entry(code_str)
            .or_insert_with(|| json!({ "description": status_description(code) }));
    }
    if route.auth.is_some() {
        responses
            .entry("401".to_string())
            .or_insert_with(|| json!({ "description": "Unauthorized" }));
    }
    if !route.allow.is_empty() {
        responses
            .entry("403".to_string())
            .or_insert_with(|| json!({ "description": "Forbidden" }));
    }
    // Schema-bound routes always include 422 (payload validation)
    if route.schema.is_some() && has_payload_binding(route) {
        responses
            .entry("422".to_string())
            .or_insert_with(|| json!({ "description": "Unprocessable Entity" }));
    }
    // Fallback: if no responses were found, emit a generic 200
    if responses.is_empty() {
        responses.insert(
            "200".to_string(),
            json!({
                "description": "Success",
                "content": { "application/json": { "schema": generic_json_object_schema() } }
            }),
        );
    }

    let mut op = json!({
        "tags": [tag],
        "responses": responses
    });

    if !parameters.is_empty() {
        op["parameters"] = json!(parameters);
    }
    if let Some(auth) = &route.auth {
        op["security"] = json!([{ auth.provider.clone(): [] }]);
    }

    op
}

#[derive(Debug, Clone)]
struct ResponseSpec {
    code: Option<u16>,
    schema_name: Option<String>,
    content_type: ReplyContentType,
    body: Expression,
}

#[derive(Debug, Default)]
struct RouteOutcomes {
    responses: Vec<ResponseSpec>,
    error_codes: Vec<u16>,
}

fn response_to_openapi(response: &ResponseSpec, component_names: &HashSet<String>) -> Json {
    let description = response
        .code
        .map(status_description)
        .unwrap_or("Dynamic response status");

    let mut obj = match response.content_type {
        ReplyContentType::Json => {
            let schema = response
                .schema_name
                .as_deref()
                .map(|name| schema_ref_or_fallback(name, component_names))
                .unwrap_or_else(|| infer_expression_schema(&response.body, component_names));
            json!({
                "description": description,
                "content": { "application/json": { "schema": schema } }
            })
        }
        ReplyContentType::Html => json!({
            "description": description,
            "content": { "text/html": { "schema": { "type": "string" } } }
        }),
        ReplyContentType::Text => json!({
            "description": description,
            "content": { "text/plain": { "schema": { "type": "string" } } }
        }),
    };

    if response.code.is_none() {
        obj["x-marreta-dynamic-status"] = json!(true);
    }
    obj
}

fn schema_ref_or_fallback(schema_name: &str, component_names: &HashSet<String>) -> Json {
    if component_names.contains(schema_name) {
        json!({ "$ref": format!("#/components/schemas/{}", schema_name) })
    } else {
        json!({
            "type": "object",
            "x-marreta-unresolved-schema": schema_name
        })
    }
}

fn infer_expression_schema(expr: &Expression, component_names: &HashSet<String>) -> Json {
    match expr {
        Expression::StringLiteral(_) => json!({ "type": "string" }),
        Expression::Integer(_) => json!({ "type": "integer", "format": "int64" }),
        Expression::Float(_) => json!({ "type": "number", "format": "float" }),
        Expression::Boolean(_) => json!({ "type": "boolean" }),
        Expression::Null => json!({ "nullable": true }),
        Expression::List(items) => {
            let item_schema = infer_list_item_schema(items, component_names);
            json!({ "type": "array", "items": item_schema })
        }
        Expression::MapLiteral(items) => {
            let mut properties = serde_json::Map::new();
            for (key, value) in items {
                properties.insert(key.clone(), infer_expression_schema(value, component_names));
            }
            json!({
                "type": "object",
                "properties": properties
            })
        }
        Expression::SchemaConstructor { schema_name, .. } => {
            schema_ref_or_fallback(schema_name, component_names)
        }
        Expression::FunctionCall { name, arguments }
            if name == "decimal" && !arguments.is_empty() =>
        {
            json!({ "type": "string", "format": "decimal" })
        }
        _ => generic_json_object_schema(),
    }
}

fn infer_list_item_schema(items: &[Expression], component_names: &HashSet<String>) -> Json {
    let Some(first) = items.first() else {
        return json!({});
    };
    let first_schema = infer_expression_schema(first, component_names);
    if items
        .iter()
        .skip(1)
        .all(|item| infer_expression_schema(item, component_names) == first_schema)
    {
        first_schema
    } else {
        json!({})
    }
}

fn generic_json_object_schema() -> Json {
    json!({ "type": "object" })
}

fn free_form_json_object_schema() -> Json {
    json!({ "type": "object", "additionalProperties": true })
}

fn build_request_body(route: &RouteDefinition, component_names: &HashSet<String>) -> Option<Json> {
    if has_payload_binding(route) {
        let schema = route
            .schema
            .as_deref()
            .map(|name| schema_ref_or_fallback(name, component_names))
            .unwrap_or_else(free_form_json_object_schema);
        return Some(json!({
            "required": true,
            "content": {
                "application/json": {
                    "schema": schema
                }
            }
        }));
    }
    if has_raw_binding(route) {
        return Some(json!({
            "required": true,
            "content": {
                "text/plain": {
                    "schema": { "type": "string" }
                }
            }
        }));
    }
    if has_form_binding(route) {
        return Some(json!({
            "required": true,
            "content": {
                "application/x-www-form-urlencoded": {
                    "schema": generic_json_object_schema()
                }
            }
        }));
    }
    None
}

fn operation_id(route: &RouteDefinition, seen: &mut HashMap<String, usize>) -> String {
    let mut id = verb_str(&route.verb).to_string();
    for segment in route.path.split('/').filter(|segment| !segment.is_empty()) {
        id.push('_');
        if let Some(param) = segment.strip_prefix(':') {
            id.push_str("by_");
            id.push_str(&normalize_operation_id_segment(param));
        } else {
            id.push_str(&normalize_operation_id_segment(segment));
        }
    }
    let id = collapse_underscores(&id);
    let count = seen.entry(id.clone()).or_insert(0);
    *count += 1;
    if *count == 1 {
        id
    } else {
        format!("{}_{}", id, count)
    }
}

fn normalize_operation_id_segment(segment: &str) -> String {
    segment
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect()
}

fn collapse_underscores(input: &str) -> String {
    let mut out = String::new();
    let mut previous_underscore = false;
    for ch in input.chars() {
        if ch == '_' {
            if !previous_underscore {
                out.push(ch);
            }
            previous_underscore = true;
        } else {
            out.push(ch);
            previous_underscore = false;
        }
    }
    out.trim_matches('_').to_string()
}

fn build_security_schemes(auth_providers: &HashMap<String, AuthProvider>) -> HashMap<String, Json> {
    let mut schemes = HashMap::new();
    for (name, provider) in auth_providers {
        match provider {
            AuthProvider::Jwt(_) => {
                schemes.insert(
                    name.clone(),
                    json!({
                        "type": "http",
                        "scheme": "bearer",
                        "bearerFormat": "JWT"
                    }),
                );
            }
            AuthProvider::ApiKey(config) => {
                let header = config
                    .fields
                    .iter()
                    .find(|field| field.name == "header")
                    .and_then(|field| expression_to_static_string(&field.value))
                    .unwrap_or_else(|| "api-key".to_string());
                schemes.insert(
                    name.clone(),
                    json!({
                        "type": "apiKey",
                        "in": "header",
                        "name": header
                    }),
                );
            }
        }
    }
    schemes
}

fn expression_to_static_string(expr: &Expression) -> Option<String> {
    match expr {
        Expression::StringLiteral(value) => Some(value.clone()),
        Expression::PropertyAccess { object, property } if matches!(object.as_ref(), Expression::Identifier(name) if name == "env") => {
            std::env::var(property).ok()
        }
        _ => None,
    }
}

fn collect_route_outcomes(body: &[Statement]) -> RouteOutcomes {
    let mut outcomes = RouteOutcomes::default();
    for stmt in body {
        collect_statement_outcomes(stmt, &mut outcomes);
    }
    dedupe_outcomes(outcomes)
}

fn collect_statement_outcomes(stmt: &Statement, outcomes: &mut RouteOutcomes) {
    match stmt {
        Statement::Reply {
            status_code,
            response_schema,
            content_type,
            body,
            ..
        } => outcomes.responses.push(ResponseSpec {
            code: literal_status_code(status_code),
            schema_name: response_schema.clone(),
            content_type: content_type.clone(),
            body: body.clone(),
        }),
        Statement::Fail { status_code, .. }
        | Statement::Require {
            error_code: status_code,
            ..
        }
        | Statement::Reject {
            error_code: status_code,
            ..
        } => outcomes.error_codes.push(*status_code as u16),
        Statement::Raise { .. } => outcomes.error_codes.push(500),
        Statement::Export(inner) => collect_statement_outcomes(inner, outcomes),
        Statement::Transaction { body, .. }
        | Statement::While { body, .. }
        | Statement::OnQueue { body, .. }
        | Statement::OnTopic { body, .. } => collect_statement_list_outcomes(body, outcomes),
        Statement::TaskDef { body, .. } => collect_task_body_outcomes(body, outcomes),
        Statement::ExpressionStatement { expression, .. }
        | Statement::Assignment {
            value: expression, ..
        }
        | Statement::ConditionalAssignment {
            value: expression, ..
        } => collect_expression_outcomes(expression, outcomes),
        _ => {}
    }
}

fn collect_statement_list_outcomes(statements: &[Statement], outcomes: &mut RouteOutcomes) {
    for stmt in statements {
        collect_statement_outcomes(stmt, outcomes);
    }
}

fn collect_task_body_outcomes(body: &TaskBody, outcomes: &mut RouteOutcomes) {
    match body {
        TaskBody::Inline(expr) => collect_expression_outcomes(expr, outcomes),
        TaskBody::Block(statements, expr) => {
            collect_statement_list_outcomes(statements, outcomes);
            collect_expression_outcomes(expr, outcomes);
        }
    }
}

fn collect_expression_outcomes(expr: &Expression, outcomes: &mut RouteOutcomes) {
    match expr {
        Expression::If {
            condition,
            then_branch,
            else_branch,
        } => {
            collect_expression_outcomes(condition, outcomes);
            collect_task_body_outcomes(then_branch, outcomes);
            if let Some(else_branch) = else_branch {
                collect_task_body_outcomes(else_branch, outcomes);
            }
        }
        Expression::Pipeline { input, stages } => {
            collect_expression_outcomes(input, outcomes);
            for stage in stages {
                collect_pipeline_stage_outcomes(stage, outcomes);
            }
        }
        Expression::Rescue { expr, handler } => {
            collect_expression_outcomes(expr, outcomes);
            collect_expression_outcomes(handler, outcomes);
        }
        Expression::List(items) => {
            for item in items {
                collect_expression_outcomes(item, outcomes);
            }
        }
        Expression::MapLiteral(items) | Expression::SchemaConstructor { fields: items, .. } => {
            for (_, value) in items {
                collect_expression_outcomes(value, outcomes);
            }
        }
        Expression::BinaryOp { left, right, .. } => {
            collect_expression_outcomes(left, outcomes);
            collect_expression_outcomes(right, outcomes);
        }
        Expression::UnaryOp { operand, .. }
        | Expression::PropertyAccess {
            object: operand, ..
        } => {
            collect_expression_outcomes(operand, outcomes);
        }
        Expression::MethodCall {
            object, arguments, ..
        } => {
            collect_expression_outcomes(object, outcomes);
            collect_argument_outcomes(arguments, outcomes);
        }
        Expression::HttpClientResponseSchema { call, .. } => {
            collect_expression_outcomes(call, outcomes)
        }
        Expression::FunctionCall { name, arguments } => {
            if name == "__fail__"
                && let Some(Argument::Positional(Expression::Integer(code))) = arguments.first()
            {
                outcomes.error_codes.push(*code as u16);
            }
            collect_argument_outcomes(arguments, outcomes);
        }
        Expression::Match { subject, arms } => {
            collect_expression_outcomes(subject, outcomes);
            for arm in arms {
                collect_expression_outcomes(&arm.value, outcomes);
            }
        }
        Expression::Subscript { object, key } => {
            collect_expression_outcomes(object, outcomes);
            collect_expression_outcomes(key, outcomes);
        }
        Expression::Broadcast { input, targets } => {
            collect_expression_outcomes(input, outcomes);
            for target in targets {
                collect_expression_outcomes(target, outcomes);
            }
        }
        Expression::QueuePush {
            queue_name,
            payload,
            ..
        } => {
            collect_expression_outcomes(queue_name, outcomes);
            if let Some(payload) = payload {
                collect_expression_outcomes(payload, outcomes);
            }
        }
        Expression::TopicPublish { topic, payload, .. } => {
            collect_expression_outcomes(topic, outcomes);
            if let Some(payload) = payload {
                collect_expression_outcomes(payload, outcomes);
            }
        }
        _ => {}
    }
}

fn collect_argument_outcomes(arguments: &[Argument], outcomes: &mut RouteOutcomes) {
    for arg in arguments {
        match arg {
            Argument::Positional(expr) | Argument::Named { value: expr, .. } => {
                collect_expression_outcomes(expr, outcomes)
            }
        }
    }
}

fn collect_pipeline_stage_outcomes(stage: &PipelineStage, outcomes: &mut RouteOutcomes) {
    match stage {
        PipelineStage::Expression(expr) => collect_expression_outcomes(expr, outcomes),
        PipelineStage::Map { body, .. } => {
            for stmt in body {
                match stmt {
                    MapStatement::Statement(stmt) => collect_statement_outcomes(stmt, outcomes),
                    MapStatement::Keep { value, condition } => {
                        collect_expression_outcomes(value, outcomes);
                        if let Some(condition) = condition {
                            collect_expression_outcomes(condition, outcomes);
                        }
                    }
                    MapStatement::Skip { condition } => {
                        collect_expression_outcomes(condition, outcomes);
                    }
                }
            }
        }
        PipelineStage::Reduce { initial, body, .. } => {
            collect_expression_outcomes(initial, outcomes);
            collect_task_body_outcomes(body, outcomes);
        }
        PipelineStage::Rescue { handler } => collect_rescue_handler_outcomes(handler, outcomes),
    }
}

fn collect_rescue_handler_outcomes(handler: &RescueHandler, outcomes: &mut RouteOutcomes) {
    match handler {
        RescueHandler::Inline(expr) => collect_expression_outcomes(expr, outcomes),
        RescueHandler::Block(statements) => collect_statement_list_outcomes(statements, outcomes),
    }
}

fn literal_status_code(expr: &Expression) -> Option<u16> {
    match expr {
        Expression::Integer(n) if *n >= 100 && *n <= 599 => Some(*n as u16),
        _ => None,
    }
}

fn dedupe_outcomes(mut outcomes: RouteOutcomes) -> RouteOutcomes {
    let mut seen_responses: HashSet<String> = HashSet::new();
    outcomes.responses.retain(|response| {
        let key = response
            .code
            .map(|code| code.to_string())
            .unwrap_or_else(|| "default".to_string());
        seen_responses.insert(key)
    });
    outcomes.error_codes.sort_unstable();
    outcomes.error_codes.dedup();
    outcomes
}

/// Returns a human-readable description for common HTTP status codes.
fn status_description(code: u16) -> &'static str {
    match code {
        200 => "Success",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        410 => "Gone",
        422 => "Unprocessable Entity",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "Response",
    }
}

fn has_payload_binding(route: &RouteDefinition) -> bool {
    route
        .take
        .iter()
        .any(|b| matches!(b, TakeBinding::Payload(_)))
}

fn has_raw_binding(route: &RouteDefinition) -> bool {
    route.take.iter().any(|b| matches!(b, TakeBinding::Raw(_)))
}

fn has_form_binding(route: &RouteDefinition) -> bool {
    route.take.iter().any(|b| matches!(b, TakeBinding::Form(_)))
}

fn has_query_binding(route: &RouteDefinition) -> bool {
    route
        .take
        .iter()
        .any(|b| matches!(b, TakeBinding::Query(_)))
}

fn has_headers_binding(route: &RouteDefinition) -> bool {
    route
        .take
        .iter()
        .any(|b| matches!(b, TakeBinding::Headers(_)))
}

/// Converts a file stem (`schema_test`) to a human-readable tag (`Schema Test`).
fn stem_to_tag(stem: &str) -> String {
    stem.split('_')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Converts a MarretaLang path (`:param`) to OpenAPI path (`{param}`).
fn to_openapi_path(path: &str) -> String {
    path.split('/')
        .map(|seg| {
            if let Some(name) = seg.strip_prefix(':') {
                format!("{{{}}}", name)
            } else {
                seg.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn verb_str(verb: &HttpVerb) -> &'static str {
    match verb {
        HttpVerb::Get => "get",
        HttpVerb::Post => "post",
        HttpVerb::Put => "put",
        HttpVerb::Patch => "patch",
        HttpVerb::Delete => "delete",
    }
}

/// Maps a `SchemaType` to an OpenAPI JSON property object.
///
/// Primitives return `{ "type": "..." }` (with optional format).
/// `Reference` and `TypedList` are fully handled in Phase 4 (OpenAPI Refinement).
/// For now they fall back to safe defaults so the server always starts.
fn schema_type_to_openapi(t: &SchemaType) -> Json {
    match t {
        SchemaType::StringType => json!({ "type": "string" }),
        SchemaType::IntegerType => json!({ "type": "integer", "format": "int64" }),
        SchemaType::FloatType => json!({ "type": "number", "format": "float" }),
        SchemaType::DecimalType => json!({ "type": "string", "format": "decimal" }),
        SchemaType::BooleanType => json!({ "type": "boolean" }),
        SchemaType::InstantType => json!({ "type": "string", "format": "date-time" }),
        SchemaType::DateType => json!({ "type": "string", "format": "date" }),
        SchemaType::TimeType => json!({ "type": "string", "format": "time" }),
        SchemaType::DurationType => json!({ "type": "string", "format": "duration" }),
        SchemaType::IntervalType => json!({
            "type": "object",
            "properties": {
                "start": { "type": "string" },
                "end": { "type": "string" }
            },
            "required": ["start", "end"]
        }),
        SchemaType::ListType => json!({ "type": "array" }),
        SchemaType::MapType => json!({ "type": "object" }),
        SchemaType::EnumType(values) => json!({ "type": "string", "enum": values }),
        SchemaType::Reference(name) => {
            json!({ "$ref": format!("#/components/schemas/{}", name) })
        }
        SchemaType::TypedList(inner) => {
            json!({ "type": "array", "items": schema_type_to_openapi(inner) })
        }
    }
}

/// Extracts the string value from a `StringLiteral` expression, returning `None` for dynamic exprs.
fn extract_string_literal(expr: &Expression) -> Option<String> {
    match expr {
        Expression::StringLiteral(s) => Some(s.clone()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{
        AuthProviderConfig, AuthProviderField, Expression, HttpVerb, ReplyContentType, RouteAuth,
        SchemaField, SchemaType, Statement, TakeBinding,
    };
    use crate::environment::Environment;
    use crate::file_loader::{ModuleRuntime, ProjectRuntime};
    use crate::route_loader::{RouteDefinition, RouteRegistry, SchemaDefinition};

    fn empty_registry() -> RouteRegistry {
        RouteRegistry {
            routes: vec![],
            schemas: HashMap::new(),
            persistent_schemas: HashMap::new(),
            startup_stmts: vec![],
            consumers: vec![],
            auth_providers: HashMap::new(),
        }
    }

    fn parse_spec(registry: &RouteRegistry) -> Json {
        let s = build(registry, "Test API", "1.0.0");
        serde_json::from_str(&s).unwrap()
    }

    fn parse_spec_with_runtime(registry: &RouteRegistry, runtime: &ProjectRuntime) -> Json {
        let s = build_with_runtime(registry, runtime, "Test API", "1.0.0");
        serde_json::from_str(&s).unwrap()
    }

    fn auth_field(name: &str, value: Expression) -> AuthProviderField {
        AuthProviderField {
            name: name.into(),
            value,
            line: 1,
            column: 1,
        }
    }

    #[test]
    fn test_basic_structure() {
        let spec = parse_spec(&empty_registry());
        assert_eq!(spec["openapi"], "3.0.3");
        assert_eq!(spec["info"]["title"], "Test API");
        assert_eq!(spec["info"]["version"], "1.0.0");
        assert!(spec["paths"].is_object());
        assert_eq!(spec["tags"][0]["name"], "Test API");
    }

    #[test]
    fn test_get_route_appears_in_paths() {
        let mut registry = empty_registry();
        registry.routes.push(RouteDefinition {
            verb: HttpVerb::Get,
            path: "/health".into(),
            auth: None,
            allow: vec![],
            take: vec![],
            schema: None,
            body: vec![],
            line: 1,
            column: 1,
            source_file: None,
            module_id: None,
        });
        let spec = parse_spec(&registry);
        assert!(spec["paths"]["/health"]["get"].is_object());
    }

    #[test]
    fn test_path_param_extracted() {
        let mut registry = empty_registry();
        registry.routes.push(RouteDefinition {
            verb: HttpVerb::Get,
            path: "/users/:id".into(),
            auth: None,
            allow: vec![],
            take: vec![],
            schema: None,
            body: vec![],
            line: 1,
            column: 1,
            source_file: None,
            module_id: None,
        });
        let spec = parse_spec(&registry);
        let params = &spec["paths"]["/users/{id}"]["get"]["parameters"];
        assert!(params.is_array());
        assert_eq!(params[0]["name"], "id");
        assert_eq!(params[0]["in"], "path");
    }

    #[test]
    fn test_schema_in_components() {
        let mut registry = empty_registry();
        registry.schemas.insert(
            "user_payload".into(),
            SchemaDefinition {
                db_table: None,
                fields: vec![
                    SchemaField {
                        name: "name".into(),
                        field_type: SchemaType::StringType,
                        optional: false,
                    },
                    SchemaField {
                        name: "age".into(),
                        field_type: SchemaType::IntegerType,
                        optional: false,
                    },
                    SchemaField {
                        name: "email".into(),
                        field_type: SchemaType::StringType,
                        optional: true,
                    },
                ],
            },
        );
        let spec = parse_spec(&registry);
        let schema = &spec["components"]["schemas"]["user_payload"];
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["properties"]["name"]["type"], "string");
        assert_eq!(schema["properties"]["age"]["type"], "integer");
        // required list contains only non-optional fields
        let required = schema["required"].as_array().unwrap();
        assert!(required.contains(&json!("name")));
        assert!(required.contains(&json!("age")));
        assert!(!required.contains(&json!("email")));
    }

    #[test]
    fn test_temporal_schema_types_map_to_openapi_shapes() {
        let mut registry = empty_registry();
        registry.schemas.insert(
            "time_payload".into(),
            SchemaDefinition {
                db_table: None,
                fields: vec![
                    SchemaField {
                        name: "created_at".into(),
                        field_type: SchemaType::InstantType,
                        optional: false,
                    },
                    SchemaField {
                        name: "billing_date".into(),
                        field_type: SchemaType::DateType,
                        optional: false,
                    },
                    SchemaField {
                        name: "opens_at".into(),
                        field_type: SchemaType::TimeType,
                        optional: false,
                    },
                    SchemaField {
                        name: "sla".into(),
                        field_type: SchemaType::DurationType,
                        optional: false,
                    },
                    SchemaField {
                        name: "business_window".into(),
                        field_type: SchemaType::IntervalType,
                        optional: false,
                    },
                ],
            },
        );

        let spec = parse_spec(&registry);
        let schema = &spec["components"]["schemas"]["time_payload"]["properties"];

        assert_eq!(schema["created_at"]["type"], "string");
        assert_eq!(schema["created_at"]["format"], "date-time");
        assert_eq!(schema["billing_date"]["format"], "date");
        assert_eq!(schema["opens_at"]["format"], "time");
        assert_eq!(schema["sla"]["format"], "duration");
        assert_eq!(schema["business_window"]["type"], "object");
        assert_eq!(
            schema["business_window"]["required"],
            json!(["start", "end"])
        );
    }

    #[test]
    fn test_api_contract_types_map_to_openapi_shapes() {
        let mut registry = empty_registry();
        registry.schemas.insert(
            "payment_payload".into(),
            SchemaDefinition {
                db_table: None,
                fields: vec![
                    SchemaField {
                        name: "status".into(),
                        field_type: SchemaType::EnumType(vec!["pending".into(), "paid".into()]),
                        optional: false,
                    },
                    SchemaField {
                        name: "amount".into(),
                        field_type: SchemaType::DecimalType,
                        optional: false,
                    },
                ],
            },
        );

        let spec = parse_spec(&registry);
        let schema = &spec["components"]["schemas"]["payment_payload"]["properties"];

        assert_eq!(schema["status"]["type"], "string");
        assert_eq!(schema["status"]["enum"], json!(["pending", "paid"]));
        assert_eq!(schema["amount"]["type"], "string");
        assert_eq!(schema["amount"]["format"], "decimal");
    }

    #[test]
    fn test_request_body_ref_when_schema_bound() {
        let mut registry = empty_registry();
        registry.schemas.insert(
            "user_payload".into(),
            SchemaDefinition {
                db_table: None,
                fields: vec![],
            },
        );
        registry.routes.push(RouteDefinition {
            verb: HttpVerb::Post,
            path: "/users".into(),
            auth: None,
            allow: vec![],
            take: vec![TakeBinding::Payload("payload".into())],
            schema: Some("user_payload".into()),
            body: vec![],
            line: 1,
            column: 1,
            source_file: None,
            module_id: None,
        });
        let spec = parse_spec(&registry);
        let ref_val = &spec["paths"]["/users"]["post"]["requestBody"]["content"]["application/json"]
            ["schema"]["$ref"];
        assert_eq!(ref_val, "#/components/schemas/user_payload");
    }

    #[test]
    fn test_raw_binding_emits_text_request_body() {
        let mut registry = empty_registry();
        registry.routes.push(RouteDefinition {
            verb: HttpVerb::Post,
            path: "/raw".into(),
            auth: None,
            allow: vec![],
            take: vec![TakeBinding::Raw("raw".into())],
            schema: None,
            body: vec![],
            line: 1,
            column: 1,
            source_file: None,
            module_id: None,
        });

        let spec = parse_spec(&registry);
        let schema =
            &spec["paths"]["/raw"]["post"]["requestBody"]["content"]["text/plain"]["schema"];
        assert_eq!(schema["type"], "string");
    }

    #[test]
    fn test_private_route_schema_is_included_when_runtime_can_resolve_it() {
        let mut registry = empty_registry();
        registry.routes.push(RouteDefinition {
            verb: HttpVerb::Post,
            path: "/private".into(),
            auth: None,
            allow: vec![],
            take: vec![TakeBinding::Payload("payload".into())],
            schema: Some("PrivatePayload".into()),
            body: vec![],
            line: 1,
            column: 1,
            source_file: Some("private".into()),
            module_id: Some("routes/private".into()),
        });

        let private_schema = SchemaDefinition {
            db_table: None,
            fields: vec![SchemaField {
                name: "message".into(),
                field_type: SchemaType::StringType,
                optional: false,
            }],
        };
        let mut visible_schemas = HashMap::new();
        visible_schemas.insert("PrivatePayload".into(), private_schema);
        let mut modules = HashMap::new();
        modules.insert(
            "routes/private".into(),
            ModuleRuntime {
                id: "routes/private".into(),
                env: Environment::new(),
                visible_schemas,
            },
        );
        let runtime = ProjectRuntime {
            global_env: Environment::new(),
            modules,
            public_schemas: HashMap::new(),
            persistent_schemas: HashMap::new(),
            feature_flags: Default::default(),
            task_namespaces: HashMap::new(),
            db_columns: HashMap::new(),
        };

        let spec = parse_spec_with_runtime(&registry, &runtime);
        assert_eq!(
            spec["components"]["schemas"]["PrivatePayload"]["properties"]["message"]["type"],
            "string"
        );
        assert_eq!(
            spec["paths"]["/private"]["post"]["requestBody"]["content"]["application/json"]["schema"]
                ["$ref"],
            "#/components/schemas/PrivatePayload"
        );
    }

    #[test]
    fn test_jwt_auth_provider_generates_bearer_security_scheme() {
        let mut registry = empty_registry();
        registry.auth_providers.insert(
            "customer_auth".into(),
            AuthProvider::Jwt(AuthProviderConfig {
                name: "customer_auth".into(),
                fields: vec![
                    auth_field(
                        "issuer",
                        Expression::StringLiteral("https://issuer.example.test".into()),
                    ),
                    auth_field("audience", Expression::StringLiteral("shop-api".into())),
                ],
            }),
        );
        let spec = parse_spec(&registry);
        let scheme = &spec["components"]["securitySchemes"]["customer_auth"];
        assert_eq!(scheme["type"], "http");
        assert_eq!(scheme["scheme"], "bearer");
        assert_eq!(scheme["bearerFormat"], "JWT");
    }

    #[test]
    fn test_api_key_auth_provider_generates_header_security_scheme() {
        let mut registry = empty_registry();
        registry.auth_providers.insert(
            "internal_auth".into(),
            AuthProvider::ApiKey(AuthProviderConfig {
                name: "internal_auth".into(),
                fields: vec![
                    auth_field("header", Expression::StringLiteral("x-api-key".into())),
                    auth_field("secret_hash", Expression::StringLiteral("hash".into())),
                ],
            }),
        );
        let spec = parse_spec(&registry);
        let scheme = &spec["components"]["securitySchemes"]["internal_auth"];
        assert_eq!(scheme["type"], "apiKey");
        assert_eq!(scheme["in"], "header");
        assert_eq!(scheme["name"], "x-api-key");
    }

    #[test]
    fn test_protected_route_includes_security_requirement() {
        let mut registry = empty_registry();
        registry.auth_providers.insert(
            "customer_auth".into(),
            AuthProvider::Jwt(AuthProviderConfig {
                name: "customer_auth".into(),
                fields: vec![
                    auth_field(
                        "issuer",
                        Expression::StringLiteral("https://issuer.example.test".into()),
                    ),
                    auth_field("audience", Expression::StringLiteral("shop-api".into())),
                ],
            }),
        );
        registry.routes.push(RouteDefinition {
            verb: HttpVerb::Get,
            path: "/orders".into(),
            auth: Some(RouteAuth {
                provider: "customer_auth".into(),
                line: 1,
                column: 5,
            }),
            allow: vec![],
            take: vec![],
            schema: None,
            body: vec![],
            line: 1,
            column: 1,
            source_file: None,
            module_id: None,
        });
        let spec = parse_spec(&registry);
        assert_eq!(
            spec["paths"]["/orders"]["get"]["security"][0]["customer_auth"],
            json!([])
        );
        assert!(spec["paths"]["/orders"]["get"]["responses"]["401"].is_object());
    }

    #[test]
    fn test_allow_route_includes_forbidden_response() {
        let mut registry = empty_registry();
        registry.routes.push(RouteDefinition {
            verb: HttpVerb::Get,
            path: "/owned".into(),
            auth: None,
            allow: vec![Expression::Boolean(true)],
            take: vec![],
            schema: None,
            body: vec![reply_stmt(200, None)],
            line: 1,
            column: 1,
            source_file: None,
            module_id: None,
        });

        let spec = parse_spec(&registry);
        assert!(spec["paths"]["/owned"]["get"]["responses"]["403"].is_object());
    }

    #[test]
    fn test_public_route_has_no_security_requirement() {
        let mut registry = empty_registry();
        registry.auth_providers.insert(
            "customer_auth".into(),
            AuthProvider::Jwt(AuthProviderConfig {
                name: "customer_auth".into(),
                fields: vec![
                    auth_field(
                        "issuer",
                        Expression::StringLiteral("https://issuer.example.test".into()),
                    ),
                    auth_field("audience", Expression::StringLiteral("shop-api".into())),
                ],
            }),
        );
        registry.routes.push(RouteDefinition {
            verb: HttpVerb::Get,
            path: "/public".into(),
            auth: None,
            allow: vec![],
            take: vec![],
            schema: None,
            body: vec![],
            line: 1,
            column: 1,
            source_file: None,
            module_id: None,
        });
        let spec = parse_spec(&registry);
        assert!(spec["paths"]["/public"]["get"].get("security").is_none());
    }

    #[test]
    fn test_payload_without_schema_emits_generic_request_body() {
        let mut registry = empty_registry();
        registry.routes.push(RouteDefinition {
            verb: HttpVerb::Post,
            path: "/echo".into(),
            auth: None,
            allow: vec![],
            take: vec![TakeBinding::Payload("payload".into())],
            schema: None,
            body: vec![],
            line: 1,
            column: 1,
            source_file: None,
            module_id: None,
        });
        let spec = parse_spec(&registry);
        let schema =
            &spec["paths"]["/echo"]["post"]["requestBody"]["content"]["application/json"]["schema"];
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["additionalProperties"], true);
    }

    #[test]
    fn test_no_components_when_no_schemas() {
        let spec = parse_spec(&empty_registry());
        assert!(spec["components"].is_null());
    }

    #[test]
    fn test_all_http_verbs() {
        let verbs = [
            (HttpVerb::Get, "get"),
            (HttpVerb::Post, "post"),
            (HttpVerb::Put, "put"),
            (HttpVerb::Patch, "patch"),
            (HttpVerb::Delete, "delete"),
        ];
        for (verb, expected) in verbs {
            let mut registry = empty_registry();
            registry.routes.push(RouteDefinition {
                verb,
                path: "/test".into(),
                auth: None,
                allow: vec![],
                take: vec![],
                schema: None,
                body: vec![],
                line: 1,
                column: 1,
                source_file: None,
                module_id: None,
            });
            let spec = parse_spec(&registry);
            assert!(
                spec["paths"]["/test"][expected].is_object(),
                "missing verb {}",
                expected
            );
        }
    }

    #[test]
    fn test_schema_type_mappings() {
        assert_eq!(
            schema_type_to_openapi(&SchemaType::StringType),
            json!({ "type": "string" })
        );
        assert_eq!(
            schema_type_to_openapi(&SchemaType::IntegerType),
            json!({ "type": "integer", "format": "int64" })
        );
        assert_eq!(
            schema_type_to_openapi(&SchemaType::FloatType),
            json!({ "type": "number", "format": "float" })
        );
        assert_eq!(
            schema_type_to_openapi(&SchemaType::BooleanType),
            json!({ "type": "boolean" })
        );
        assert_eq!(
            schema_type_to_openapi(&SchemaType::ListType),
            json!({ "type": "array" })
        );
        assert_eq!(
            schema_type_to_openapi(&SchemaType::MapType),
            json!({ "type": "object" })
        );
    }

    #[test]
    fn test_schema_type_reference() {
        let result = schema_type_to_openapi(&SchemaType::Reference("address".into()));
        assert_eq!(result["$ref"], "#/components/schemas/address");
    }

    #[test]
    fn test_schema_type_typed_list_primitive() {
        let result =
            schema_type_to_openapi(&SchemaType::TypedList(Box::new(SchemaType::StringType)));
        assert_eq!(result["type"], "array");
        assert_eq!(result["items"]["type"], "string");
    }

    #[test]
    fn test_schema_type_typed_list_reference() {
        let result = schema_type_to_openapi(&SchemaType::TypedList(Box::new(
            SchemaType::Reference("order_item".into()),
        )));
        assert_eq!(result["type"], "array");
        assert_eq!(result["items"]["$ref"], "#/components/schemas/order_item");
    }

    fn reply_stmt(status_code: i64, response_schema: Option<&str>) -> Statement {
        Statement::Reply {
            status_code: Expression::Integer(status_code),
            content_type: ReplyContentType::Json,
            body: Expression::Null,
            response_schema: response_schema.map(|s| s.to_string()),
            extra_headers: None,
            line: 1,
            column: 1,
        }
    }

    fn fail_stmt(status_code: i64) -> Statement {
        Statement::Fail {
            status_code,
            message: Expression::StringLiteral("error".into()),
            line: 1,
            column: 1,
        }
    }

    #[test]
    fn test_responses_from_reply_statement() {
        let mut registry = empty_registry();
        registry.routes.push(RouteDefinition {
            verb: HttpVerb::Get,
            path: "/items".into(),
            auth: None,
            allow: vec![],
            take: vec![],
            schema: None,
            body: vec![reply_stmt(200, None)],
            line: 1,
            column: 1,
            source_file: None,
            module_id: None,
        });
        let spec = parse_spec(&registry);
        assert!(spec["paths"]["/items"]["get"]["responses"]["200"].is_object());
        assert!(spec["paths"]["/items"]["get"]["responses"]["200"]["description"].is_string());
    }

    #[test]
    fn test_response_schema_ref_in_responses() {
        let mut registry = empty_registry();
        registry.schemas.insert(
            "order_result".into(),
            SchemaDefinition {
                db_table: None,
                fields: vec![],
            },
        );
        registry.routes.push(RouteDefinition {
            verb: HttpVerb::Post,
            path: "/orders".into(),
            auth: None,
            allow: vec![],
            take: vec![],
            schema: None,
            body: vec![reply_stmt(201, Some("order_result"))],
            line: 1,
            column: 1,
            source_file: None,
            module_id: None,
        });
        let spec = parse_spec(&registry);
        let ref_val = &spec["paths"]["/orders"]["post"]["responses"]["201"]["content"]["application/json"]
            ["schema"]["$ref"];
        assert_eq!(ref_val, "#/components/schemas/order_result");
    }

    #[test]
    fn test_fail_status_in_responses() {
        let mut registry = empty_registry();
        registry.routes.push(RouteDefinition {
            verb: HttpVerb::Get,
            path: "/users/:id".into(),
            auth: None,
            allow: vec![],
            take: vec![],
            schema: None,
            body: vec![reply_stmt(200, None), fail_stmt(404)],
            line: 1,
            column: 1,
            source_file: None,
            module_id: None,
        });
        let spec = parse_spec(&registry);
        assert!(spec["paths"]["/users/{id}"]["get"]["responses"]["200"].is_object());
        assert!(spec["paths"]["/users/{id}"]["get"]["responses"]["404"].is_object());
    }

    #[test]
    fn test_422_added_for_schema_validated_route() {
        let mut registry = empty_registry();
        registry.schemas.insert(
            "order_payload".into(),
            SchemaDefinition {
                db_table: None,
                fields: vec![],
            },
        );
        registry.routes.push(RouteDefinition {
            verb: HttpVerb::Post,
            path: "/orders".into(),
            auth: None,
            allow: vec![],
            take: vec![TakeBinding::Payload("p".into())],
            schema: Some("order_payload".into()),
            body: vec![reply_stmt(201, None)],
            line: 1,
            column: 1,
            source_file: None,
            module_id: None,
        });
        let spec = parse_spec(&registry);
        assert!(spec["paths"]["/orders"]["post"]["responses"]["422"].is_object());
    }

    #[test]
    fn test_fallback_200_when_no_reply_statements() {
        let mut registry = empty_registry();
        registry.routes.push(RouteDefinition {
            verb: HttpVerb::Get,
            path: "/health".into(),
            auth: None,
            allow: vec![],
            take: vec![],
            schema: None,
            body: vec![],
            line: 1,
            column: 1,
            source_file: None,
            module_id: None,
        });
        let spec = parse_spec(&registry);
        assert!(spec["paths"]["/health"]["get"]["responses"]["200"].is_object());
    }

    fn require_stmt(error_code: i64) -> Statement {
        Statement::Require {
            condition: Expression::Null,
            error_code,
            error_message: "error".into(),
            line: 1,
            column: 1,
        }
    }

    fn reject_stmt(error_code: i64) -> Statement {
        Statement::Reject {
            condition: Expression::Null,
            error_code,
            error_message: "error".into(),
            line: 1,
            column: 1,
        }
    }

    #[test]
    fn test_query_binding_shows_query_parameter() {
        let mut registry = empty_registry();
        registry.routes.push(RouteDefinition {
            verb: HttpVerb::Get,
            path: "/search".into(),
            auth: None,
            allow: vec![],
            take: vec![TakeBinding::Query("q".into())],
            schema: None,
            body: vec![],
            line: 1,
            column: 1,
            source_file: None,
            module_id: None,
        });
        let spec = parse_spec(&registry);
        let params = &spec["paths"]["/search"]["get"]["parameters"];
        assert!(params.is_array());
        let query_param = params
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["in"] == "query");
        assert!(query_param.is_some());
        let query_param = query_param.unwrap();
        assert_eq!(query_param["style"], "deepObject");
        assert_eq!(query_param["explode"], true);
    }

    #[test]
    fn test_headers_binding_emits_marreta_extension() {
        let mut registry = empty_registry();
        registry.routes.push(RouteDefinition {
            verb: HttpVerb::Get,
            path: "/headers".into(),
            auth: None,
            allow: vec![],
            take: vec![TakeBinding::Headers("headers".into())],
            schema: None,
            body: vec![reply_stmt(200, None)],
            line: 1,
            column: 1,
            source_file: None,
            module_id: None,
        });

        let spec = parse_spec(&registry);
        assert_eq!(
            spec["paths"]["/headers"]["get"]["x-marreta-bindings"]["headers"],
            true
        );
    }

    #[test]
    fn test_dynamic_status_code_uses_default_response() {
        let mut registry = empty_registry();
        registry.routes.push(RouteDefinition {
            verb: HttpVerb::Get,
            path: "/dynamic".into(),
            auth: None,
            allow: vec![],
            take: vec![],
            schema: None,
            body: vec![Statement::Reply {
                status_code: Expression::Identifier("status_code".into()),
                content_type: ReplyContentType::Json,
                body: Expression::Null,
                response_schema: None,
                extra_headers: None,
                line: 1,
                column: 1,
            }],
            line: 1,
            column: 1,
            source_file: None,
            module_id: None,
        });
        let spec = parse_spec(&registry);
        assert!(spec["paths"]["/dynamic"]["get"]["responses"]["200"].is_null());
        assert!(spec["paths"]["/dynamic"]["get"]["responses"]["default"].is_object());
        assert_eq!(
            spec["paths"]["/dynamic"]["get"]["responses"]["default"]["x-marreta-dynamic-status"],
            true
        );
    }

    #[test]
    fn test_raise_status_in_responses() {
        let mut registry = empty_registry();
        registry.routes.push(RouteDefinition {
            verb: HttpVerb::Get,
            path: "/raise".into(),
            auth: None,
            allow: vec![],
            take: vec![],
            schema: None,
            body: vec![Statement::Raise {
                message: Expression::StringLiteral("boom".into()),
                condition: None,
                line: 1,
                column: 1,
            }],
            line: 1,
            column: 1,
            source_file: None,
            module_id: None,
        });

        let spec = parse_spec(&registry);
        assert!(spec["paths"]["/raise"]["get"]["responses"]["500"].is_object());
    }

    #[test]
    fn test_rescue_fail_status_in_responses() {
        let mut registry = empty_registry();
        registry.routes.push(RouteDefinition {
            verb: HttpVerb::Get,
            path: "/rescue".into(),
            auth: None,
            allow: vec![],
            take: vec![],
            schema: None,
            body: vec![Statement::Assignment {
                target: "result".into(),
                value: Expression::Pipeline {
                    input: Box::new(Expression::StringLiteral("input".into())),
                    stages: vec![PipelineStage::Rescue {
                        handler: RescueHandler::Inline(Expression::FunctionCall {
                            name: "__fail__".into(),
                            arguments: vec![
                                Argument::Positional(Expression::Integer(503)),
                                Argument::Positional(Expression::StringLiteral("rescued".into())),
                            ],
                        }),
                    }],
                },
                line: 1,
                column: 1,
            }],
            line: 1,
            column: 1,
            source_file: None,
            module_id: None,
        });

        let spec = parse_spec(&registry);
        assert!(spec["paths"]["/rescue"]["get"]["responses"]["503"].is_object());
    }

    #[test]
    fn test_literal_reply_body_inference() {
        let mut registry = empty_registry();
        registry.routes.push(RouteDefinition {
            verb: HttpVerb::Get,
            path: "/list".into(),
            auth: None,
            allow: vec![],
            take: vec![],
            schema: None,
            body: vec![Statement::Reply {
                status_code: Expression::Integer(200),
                content_type: ReplyContentType::Json,
                body: Expression::List(vec![Expression::Integer(1), Expression::Integer(2)]),
                response_schema: None,
                extra_headers: None,
                line: 1,
                column: 1,
            }],
            line: 1,
            column: 1,
            source_file: None,
            module_id: None,
        });

        let spec = parse_spec(&registry);
        let schema = &spec["paths"]["/list"]["get"]["responses"]["200"]["content"]["application/json"]
            ["schema"];
        assert_eq!(schema["type"], "array");
        assert_eq!(schema["items"]["type"], "integer");
    }

    #[test]
    fn test_require_status_in_responses() {
        let mut registry = empty_registry();
        registry.routes.push(RouteDefinition {
            verb: HttpVerb::Post,
            path: "/auth".into(),
            auth: None,
            allow: vec![],
            take: vec![],
            schema: None,
            body: vec![require_stmt(401), reply_stmt(200, None)],
            line: 1,
            column: 1,
            source_file: None,
            module_id: None,
        });
        let spec = parse_spec(&registry);
        assert!(spec["paths"]["/auth"]["post"]["responses"]["401"].is_object());
        assert!(spec["paths"]["/auth"]["post"]["responses"]["200"].is_object());
    }

    #[test]
    fn test_reject_status_in_responses() {
        let mut registry = empty_registry();
        registry.routes.push(RouteDefinition {
            verb: HttpVerb::Get,
            path: "/protected".into(),
            auth: None,
            allow: vec![],
            take: vec![],
            schema: None,
            body: vec![reject_stmt(403), reply_stmt(200, None)],
            line: 1,
            column: 1,
            source_file: None,
            module_id: None,
        });
        let spec = parse_spec(&registry);
        assert!(spec["paths"]["/protected"]["get"]["responses"]["403"].is_object());
    }

    #[test]
    fn test_export_wrapper_descends_into_inner() {
        let mut registry = empty_registry();
        registry.routes.push(RouteDefinition {
            verb: HttpVerb::Get,
            path: "/wrapped".into(),
            auth: None,
            allow: vec![],
            take: vec![],
            schema: None,
            body: vec![Statement::Export(Box::new(reply_stmt(201, None)))],
            line: 1,
            column: 1,
            source_file: None,
            module_id: None,
        });
        let spec = parse_spec(&registry);
        assert!(spec["paths"]["/wrapped"]["get"]["responses"]["201"].is_object());
    }

    #[test]
    fn test_status_descriptions_all_codes() {
        let codes_and_descriptions = [
            (200u16, "Success"),
            (201, "Created"),
            (202, "Accepted"),
            (204, "No Content"),
            (301, "Moved Permanently"),
            (302, "Found"),
            (400, "Bad Request"),
            (401, "Unauthorized"),
            (403, "Forbidden"),
            (404, "Not Found"),
            (405, "Method Not Allowed"),
            (409, "Conflict"),
            (410, "Gone"),
            (422, "Unprocessable Entity"),
            (429, "Too Many Requests"),
            (500, "Internal Server Error"),
            (503, "Service Unavailable"),
            (418, "Response"), // unknown → fallback
        ];
        for (code, expected) in codes_and_descriptions {
            assert_eq!(status_description(code), expected, "code {}", code);
        }
    }

    #[test]
    fn test_stem_to_tag_single_word() {
        assert_eq!(stem_to_tag("users"), "Users");
    }

    #[test]
    fn test_stem_to_tag_multiple_words() {
        assert_eq!(stem_to_tag("user_orders"), "User Orders");
    }

    #[test]
    fn test_stem_to_tag_empty() {
        assert_eq!(stem_to_tag(""), "");
    }

    #[test]
    fn test_source_file_used_as_tag() {
        let mut registry = empty_registry();
        registry.routes.push(RouteDefinition {
            verb: HttpVerb::Get,
            path: "/health".into(),
            auth: None,
            allow: vec![],
            take: vec![],
            schema: None,
            body: vec![],
            line: 1,
            column: 1,
            source_file: Some("user_routes".into()),
            module_id: None,
        });
        let spec = parse_spec(&registry);
        let tags = spec["tags"].as_array().unwrap();
        assert!(tags.iter().any(|t| t["name"] == "User Routes"));
    }

    #[test]
    fn test_operation_id_is_deterministic() {
        let mut registry = empty_registry();
        registry.routes.push(RouteDefinition {
            verb: HttpVerb::Put,
            path: "/db/items/:id".into(),
            auth: None,
            allow: vec![],
            take: vec![],
            schema: None,
            body: vec![],
            line: 1,
            column: 1,
            source_file: None,
            module_id: None,
        });

        let spec = parse_spec(&registry);
        assert_eq!(
            spec["paths"]["/db/items/{id}"]["put"]["operationId"],
            "put_db_items_by_id"
        );
    }

    #[test]
    fn test_duplicate_status_codes_deduplicated() {
        let mut registry = empty_registry();
        registry.routes.push(RouteDefinition {
            verb: HttpVerb::Get,
            path: "/items".into(),
            auth: None,
            allow: vec![],
            take: vec![],
            schema: None,
            body: vec![reply_stmt(200, None), reply_stmt(200, None)],
            line: 1,
            column: 1,
            source_file: None,
            module_id: None,
        });
        let spec = parse_spec(&registry);
        let responses = spec["paths"]["/items"]["get"]["responses"]
            .as_object()
            .unwrap();
        let count_200 = responses.keys().filter(|k| k.as_str() == "200").count();
        assert_eq!(count_200, 1);
    }

    // ─── v0.8: x-marreta-consumers ───────────────────────────────────────────

    fn make_consumer(
        kind: crate::route_loader::ConsumerKind,
        target: &str,
        schema: Option<&str>,
    ) -> crate::route_loader::ConsumerDefinition {
        use crate::ast::Expression;
        crate::route_loader::ConsumerDefinition {
            kind,
            target: Expression::StringLiteral(target.to_string()),
            binding: "msg".to_string(),
            schema: schema.map(|s| s.to_string()),
            body: vec![],
            line: 1,
            column: 1,
            source_file: None,
            module_id: None,
        }
    }

    #[test]
    fn test_no_consumers_no_extension() {
        let registry = empty_registry();
        let spec = parse_spec(&registry);
        assert!(spec.get("x-marreta-consumers").is_none());
    }

    #[test]
    fn test_queue_consumer_in_extension() {
        use crate::route_loader::ConsumerKind;
        let mut registry = empty_registry();
        registry
            .consumers
            .push(make_consumer(ConsumerKind::Queue, "orders", None));
        let spec = parse_spec(&registry);
        let consumers = spec["x-marreta-consumers"].as_array().unwrap();
        assert_eq!(consumers.len(), 1);
        assert_eq!(consumers[0]["kind"], "queue");
        assert_eq!(consumers[0]["target"], "orders");
    }

    #[test]
    fn test_topic_consumer_in_extension() {
        use crate::route_loader::ConsumerKind;
        let mut registry = empty_registry();
        registry.consumers.push(make_consumer(
            ConsumerKind::Topic,
            "payments.approved",
            None,
        ));
        let spec = parse_spec(&registry);
        let consumers = spec["x-marreta-consumers"].as_array().unwrap();
        assert_eq!(consumers[0]["kind"], "topic");
        assert_eq!(consumers[0]["pattern"], "payments.approved");
        assert!(consumers[0].get("exchange").is_none());
    }

    #[test]
    fn test_consumer_schema_in_extension() {
        use crate::route_loader::ConsumerKind;
        let mut registry = empty_registry();
        registry.consumers.push(make_consumer(
            ConsumerKind::Queue,
            "orders",
            Some("OrderPayload"),
        ));
        let spec = parse_spec(&registry);
        let consumers = spec["x-marreta-consumers"].as_array().unwrap();
        assert_eq!(consumers[0]["schema"], "OrderPayload");
    }

    #[test]
    fn test_consumer_without_schema_has_no_schema_key() {
        use crate::route_loader::ConsumerKind;
        let mut registry = empty_registry();
        registry
            .consumers
            .push(make_consumer(ConsumerKind::Queue, "orders", None));
        let spec = parse_spec(&registry);
        let consumers = spec["x-marreta-consumers"].as_array().unwrap();
        assert!(consumers[0].get("schema").is_none());
    }

    #[test]
    fn test_multiple_consumers_in_extension() {
        use crate::route_loader::ConsumerKind;
        let mut registry = empty_registry();
        registry.consumers.push(make_consumer(
            ConsumerKind::Queue,
            "orders",
            Some("OrderPayload"),
        ));
        registry
            .consumers
            .push(make_consumer(ConsumerKind::Topic, "events.new", None));
        let spec = parse_spec(&registry);
        let consumers = spec["x-marreta-consumers"].as_array().unwrap();
        assert_eq!(consumers.len(), 2);
    }
}
