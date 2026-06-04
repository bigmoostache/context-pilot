CREATE TABLE people (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    role TEXT,
    company_id INTEGER REFERENCES companies(id)
)