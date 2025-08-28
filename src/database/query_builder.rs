use sqlx::{QueryBuilder, Sqlite};

/// Utility for building dynamic SQL queries with type safety
pub struct SqlQueryBuilder {
    query: QueryBuilder<'static, Sqlite>,
}

impl SqlQueryBuilder {
    pub fn new() -> Self {
        Self {
            query: QueryBuilder::new(""),
        }
    }

    pub fn select(columns: &[&str]) -> Self {
        let mut builder = Self::new();
        builder.query.push("SELECT ");
        builder.query.push(columns.join(", "));
        builder
    }

    pub fn insert_into(table: &str) -> Self {
        let mut builder = Self::new();
        builder.query.push("INSERT INTO ");
        builder.query.push(table);
        builder
    }

    pub fn update(table: &str) -> Self {
        let mut builder = Self::new();
        builder.query.push("UPDATE ");
        builder.query.push(table);
        builder
    }

    pub fn delete_from(table: &str) -> Self {
        let mut builder = Self::new();
        builder.query.push("DELETE FROM ");
        builder.query.push(table);
        builder
    }

    pub fn from(mut self, table: &str) -> Self {
        self.query.push(" FROM ");
        self.query.push(table);
        self
    }

    pub fn values(mut self, columns: &[&str]) -> Self {
        self.query.push(" (");
        self.query.push(columns.join(", "));
        self.query.push(") VALUES (");
        for i in 0..columns.len() {
            if i > 0 {
                self.query.push(", ");
            }
            self.query.push("?");
        }
        self.query.push(")");
        self
    }

    pub fn set_columns(mut self, columns: &[&str]) -> Self {
        self.query.push(" SET ");
        for (i, col) in columns.iter().enumerate() {
            if i > 0 {
                self.query.push(", ");
            }
            self.query.push(col);
            self.query.push(" = ?");
        }
        self
    }

    pub fn where_clause(mut self, condition: &str) -> Self {
        self.query.push(" WHERE ");
        self.query.push(condition);
        self
    }

    pub fn and(mut self, condition: &str) -> Self {
        self.query.push(" AND ");
        self.query.push(condition);
        self
    }

    pub fn or(mut self, condition: &str) -> Self {
        self.query.push(" OR ");
        self.query.push(condition);
        self
    }

    pub fn order_by(mut self, column: &str, direction: &str) -> Self {
        self.query.push(" ORDER BY ");
        self.query.push(column);
        self.query.push(" ");
        self.query.push(direction);
        self
    }

    pub fn limit(mut self, count: u32) -> Self {
        self.query.push(" LIMIT ");
        self.query.push(count.to_string());
        self
    }

    pub fn returning(mut self, columns: &[&str]) -> Self {
        self.query.push(" RETURNING ");
        self.query.push(columns.join(", "));
        self
    }

    pub fn on_conflict(mut self, columns: &[&str], action: &str) -> Self {
        self.query.push(" ON CONFLICT(");
        self.query.push(columns.join(", "));
        self.query.push(") DO ");
        self.query.push(action);
        self
    }

    pub fn build(self) -> QueryBuilder<'static, Sqlite> {
        self.query
    }

    pub fn sql(&self) -> &str {
        self.query.sql()
    }
}

impl Default for SqlQueryBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Common query patterns for reducing boilerplate
pub struct CommonQueries;

impl CommonQueries {
    /// Creates a standard "get by id" query
    pub fn get_by_id(table: &str, id_column: &str, columns: &[&str]) -> SqlQueryBuilder {
        SqlQueryBuilder::select(columns)
            .from(table)
            .where_clause(&format!("{} = ?", id_column))
    }

    /// Creates a standard "insert or replace" query
    pub fn upsert(table: &str, columns: &[&str], conflict_columns: &[&str]) -> SqlQueryBuilder {
        SqlQueryBuilder::insert_into(table)
            .values(columns)
            .on_conflict(conflict_columns, "UPDATE SET")
            .returning(columns)
    }

    /// Creates a standard "update by id" query
    pub fn update_by_id(table: &str, id_column: &str, update_columns: &[&str]) -> SqlQueryBuilder {
        SqlQueryBuilder::update(table)
            .set_columns(update_columns)
            .where_clause(&format!("{} = ?", id_column))
    }

    /// Creates a standard "delete by id" query
    pub fn delete_by_id(table: &str, id_column: &str) -> SqlQueryBuilder {
        SqlQueryBuilder::delete_from(table)
            .where_clause(&format!("{} = ?", id_column))
    }
}
