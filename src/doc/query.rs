use crate::value::Value;

// ─── Accumulator ──────────────────────────────────────────────────────────────

/// A single accumulator step in a Layer 3 aggregation pipeline.
/// Each variant maps to a MongoDB `$group` accumulator expression.
#[derive(Debug, Clone)]
pub enum Accumulator {
    /// `>> sum("field", as: "alias")` → `{ "alias": { "$sum": "$field" } }`
    Sum { field: String, alias: String },
    /// `>> avg("field", as: "alias")` → `{ "alias": { "$avg": "$field" } }`
    Avg { field: String, alias: String },
    /// `>> min("field", as: "alias")` → `{ "alias": { "$min": "$field" } }`
    Min { field: String, alias: String },
    /// `>> max("field", as: "alias")` → `{ "alias": { "$max": "$field" } }`
    Max { field: String, alias: String },
    /// `>> count(as: "alias")` → `{ "alias": { "$sum": 1 } }`
    Count { alias: String },
}

// ─── Doc filter types ─────────────────────────────────────────────────────────

/// A single filter condition in a doc.* query pipeline.
/// Each variant maps to a MongoDB comparison operator.
#[derive(Debug, Clone)]
pub enum DocFilter {
    Eq(String, Value),
    Ne(String, Value),
    Gt(String, Value),
    Gte(String, Value),
    Lt(String, Value),
    Lte(String, Value),
    In(String, Vec<Value>),
    Like(String, String),
}

// ─── Sort direction ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum SortDirection {
    Asc,
    Desc,
}

// ─── Query mode ───────────────────────────────────────────────────────────────

/// Terminal mode — determines which MongoDB operation is executed.
#[derive(Debug, Clone)]
pub enum DocQueryMode {
    /// `>> fetch_all` / `>> fetch_one`
    Fetch,
    /// `>> count`
    Count,
    /// `>> exists`
    Exists,
    /// `>> update({ map })`
    Update(Value),
    /// `>> upsert({ map })`
    Upsert(Value),
    /// `>> delete`
    Delete,
    /// Aggregation pipeline active (`group_by` or any accumulator seen).
    Aggregate,
}

// ─── DocQueryState ────────────────────────────────────────────────────────────

/// Accumulated pipeline state for a lazy document query.
/// Grows as `>>` steps are added; executed only when a terminal is reached.
///
/// This is the doc.* equivalent of `db::driver::QueryState`, but is MongoDB-native:
/// no joins, no select_cols, string-based field names, different filter model.
#[derive(Debug, Clone)]
pub struct DocQueryState {
    /// Target collection name.
    pub collection: String,
    /// Accumulated filter conditions (AND-joined).
    pub filters: Vec<DocFilter>,
    /// Field projection — `>> pick(["f1", "f2"])`. Not valid in aggregation mode.
    pub projection: Option<Vec<String>>,
    /// Sort field and direction — `>> order("field", "asc"|"desc")`.
    /// In aggregation mode this is the pre-group sort (not used). Use `post_sort`.
    pub sort: Option<(String, SortDirection)>,
    /// Maximum number of documents — `>> limit(N)`.
    pub limit: Option<i64>,
    /// Number of documents to skip — `>> offset(N)`.
    pub offset: Option<i64>,
    /// Terminal mode — set when a terminal step is reached.
    pub mode: DocQueryMode,

    // ── Aggregation fields (Layer 3) ──────────────────────────────────────────
    /// `>> group_by("field")` — group key. `None` = global aggregation (`_id: null`).
    pub group_by: Option<String>,
    /// Accumulator steps: `sum`, `avg`, `min`, `max`, `count`.
    pub accumulators: Vec<Accumulator>,
    /// Post-group sort — `>> order` after accumulators.
    pub post_sort: Option<(String, SortDirection)>,
    /// Post-group limit — `>> limit` after accumulators.
    pub post_limit: Option<i64>,
}

impl DocQueryState {
    /// Creates a new query state targeting the given collection.
    pub fn new(collection: impl Into<String>) -> Self {
        Self {
            collection: collection.into(),
            filters: Vec::new(),
            projection: None,
            sort: None,
            limit: None,
            offset: None,
            mode: DocQueryMode::Fetch,
            group_by: None,
            accumulators: Vec::new(),
            post_sort: None,
            post_limit: None,
        }
    }

    /// Returns true if this query is in aggregation mode.
    pub fn is_aggregate(&self) -> bool {
        matches!(self.mode, DocQueryMode::Aggregate)
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_query_state() {
        let q = DocQueryState::new("orders");
        assert_eq!(q.collection, "orders");
        assert!(q.filters.is_empty());
        assert!(q.projection.is_none());
        assert!(q.sort.is_none());
        assert!(q.limit.is_none());
        assert!(q.offset.is_none());
        assert!(matches!(q.mode, DocQueryMode::Fetch));
    }

    #[test]
    fn test_add_filters() {
        let mut q = DocQueryState::new("users");
        q.filters
            .push(DocFilter::Eq("name".into(), Value::String("Ana".into())));
        q.filters
            .push(DocFilter::Gt("age".into(), Value::Integer(18)));
        assert_eq!(q.filters.len(), 2);
    }

    #[test]
    fn test_set_sort() {
        let mut q = DocQueryState::new("orders");
        q.sort = Some(("created_at".into(), SortDirection::Desc));
        let (field, dir) = q.sort.as_ref().unwrap();
        assert_eq!(field, "created_at");
        assert_eq!(*dir, SortDirection::Desc);
    }

    #[test]
    fn test_set_projection() {
        let mut q = DocQueryState::new("orders");
        q.projection = Some(vec!["_id".into(), "total".into(), "status".into()]);
        assert_eq!(q.projection.as_ref().unwrap().len(), 3);
    }

    #[test]
    fn test_set_limit_and_offset() {
        let mut q = DocQueryState::new("events");
        q.limit = Some(20);
        q.offset = Some(40);
        assert_eq!(q.limit, Some(20));
        assert_eq!(q.offset, Some(40));
    }

    #[test]
    fn test_mode_variants() {
        assert!(matches!(DocQueryMode::Fetch, DocQueryMode::Fetch));
        assert!(matches!(DocQueryMode::Count, DocQueryMode::Count));
        assert!(matches!(DocQueryMode::Exists, DocQueryMode::Exists));
        assert!(matches!(DocQueryMode::Delete, DocQueryMode::Delete));

        let update = DocQueryMode::Update(Value::String("test".into()));
        assert!(matches!(update, DocQueryMode::Update(_)));

        let upsert = DocQueryMode::Upsert(Value::Null);
        assert!(matches!(upsert, DocQueryMode::Upsert(_)));
    }

    #[test]
    fn test_doc_filter_variants() {
        let eq = DocFilter::Eq("status".into(), Value::String("active".into()));
        assert!(matches!(eq, DocFilter::Eq(_, _)));

        let ne = DocFilter::Ne("status".into(), Value::String("deleted".into()));
        assert!(matches!(ne, DocFilter::Ne(_, _)));

        let gt = DocFilter::Gt("total".into(), Value::Integer(100));
        assert!(matches!(gt, DocFilter::Gt(_, _)));

        let gte = DocFilter::Gte("total".into(), Value::Float(99.99));
        assert!(matches!(gte, DocFilter::Gte(_, _)));

        let lt = DocFilter::Lt("age".into(), Value::Integer(18));
        assert!(matches!(lt, DocFilter::Lt(_, _)));

        let lte = DocFilter::Lte("score".into(), Value::Float(5.0));
        assert!(matches!(lte, DocFilter::Lte(_, _)));

        let in_f = DocFilter::In(
            "role".into(),
            vec![Value::String("admin".into()), Value::String("mod".into())],
        );
        assert!(matches!(in_f, DocFilter::In(_, _)));

        let like = DocFilter::Like("email".into(), "@gmail.com".into());
        assert!(matches!(like, DocFilter::Like(_, _)));
    }

    #[test]
    fn test_sort_direction_equality() {
        assert_eq!(SortDirection::Asc, SortDirection::Asc);
        assert_eq!(SortDirection::Desc, SortDirection::Desc);
        assert_ne!(SortDirection::Asc, SortDirection::Desc);
    }
}
