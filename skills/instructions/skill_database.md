# Database Skill

You have access to SQLite database operations. Use these when the user needs to query or modify a local database.

## Available Tools

- **db_query**: Execute a read-only SQL query. Params: `db_path` (string, required), `sql` (string, required), `params` (array, optional, bind parameters). Returns result rows as JSON.
- **db_execute**: Execute a write SQL statement (INSERT, UPDATE, DELETE, CREATE, etc.). Params: `db_path` (string, required), `sql` (string, required), `params` (array, optional). Returns the number of rows affected. Requires user confirmation.
- **db_schema**: Get the schema of a database. Params: `db_path` (string, required). Returns table names, column definitions, and index information.

## Usage Guidelines

- All database paths must be absolute.
- `db_execute` requires confirmation — always show the SQL statement and explain its effect before calling it.
- Use `db_schema` to understand table structure before writing queries.
- Use parameterized queries (`params`) instead of string interpolation to avoid SQL injection.
- `db_query` is restricted to SELECT statements; use `db_execute` for any data modification.
