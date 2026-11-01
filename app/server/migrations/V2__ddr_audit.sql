-- V2: fixes from the DDR schema audit (migration/10_ddr_schema_audit.md, 2026-11-01). V1 is never edited; 04 shows
-- the schema after the last migration, and the test `schema_04_equals_the_migrations` keeps the two in step.
-- refinery runs this file in one transaction on a connection with foreign keys on.

-- GAP-02: import lineage. The runbook (03, "Extract and stage") records each export file's name, size, SHA-256, row
-- count and encoding, and UI-18 step 4 shows rows loaded and rejected per table: one row per file of a batch.
CREATE TABLE import_file (
  batch_id TEXT NOT NULL REFERENCES import_batch(id),
  source_table TEXT NOT NULL,     -- FileMaker base table name (the CSV file name without .csv)
  file_name TEXT NOT NULL,
  size_bytes INTEGER NOT NULL,
  sha256 TEXT NOT NULL,           -- lowercase hex
  encoding TEXT NOT NULL,         -- UTF-8 (OD-03-01); a file in another encoding is refused
  rows_read INTEGER NOT NULL,
  rows_loaded INTEGER NOT NULL,
  rows_rejected INTEGER NOT NULL,
  PRIMARY KEY (batch_id, source_table)
);

-- GAP-02: a value the import stored differently from the export: money rounded to the cent, a line amount recomputed
-- (BR-SALES-01), a credit_limit that is not a number (BR-IMPORT-01), a total or cache recomputed (BR-SALES-02,
-- BR-PARTY-06). The row is loaded; this is the "changed values" part of the report.
CREATE TABLE import_changed_value (
  batch_id TEXT NOT NULL REFERENCES import_batch(id),
  source_table TEXT NOT NULL,     -- FileMaker base table name
  source_key TEXT,                -- FileMaker Id of the row
  source_field TEXT NOT NULL,     -- FileMaker field name
  source_value TEXT,              -- as exported
  stored_value TEXT,              -- as stored (NULL when not stored)
  reason TEXT NOT NULL
);

CREATE INDEX ix_import_rejected_row_batch_id ON import_rejected_row (batch_id); -- report by batch (OP-IMPORT-02)
CREATE INDEX ix_import_changed_value_batch_id ON import_changed_value (batch_id); -- report by batch (OP-IMPORT-02)

-- GAP-03: the invoice page reads a transaction by its number (GET /sales/{doc_number}, invoice.rs), and the sales lists
-- and the dashboard order and filter by date.
CREATE INDEX ix_sales_transaction_doc_number ON sales_transaction (doc_number);
CREATE INDEX ix_sales_transaction_date ON sales_transaction (date);

-- GAP-01: the custom value lists whose values a target column stores (07 "Custom value lists (enum)"), in the order
-- of the DDR. list = the FileMaker value-list name; the bound columns are in 10_ddr_schema_audit.md.
INSERT INTO value_list_item (list, value, sort_order) VALUES
  ('AuthMethods', 'External Server', 1),
  ('AuthMethods', 'OAuth', 2),
  ('AuthMethods', 'FileMaker Accounts', 3),
  ('DiscussionView Choices', 'Our team only', 1),
  ('DiscussionView Choices', 'Client', 2),
  ('ItemType', 'Inventory', 1),
  ('ItemType', 'Service', 2),
  ('ItemType', 'NonInventory', 3),
  ('OpportunityProbability', 'unlikely', 1),
  ('OpportunityProbability', 'likely', 2),
  ('OpportunityProbability', 'definite', 3),
  ('OpportunityType', 'Lead', 1),
  ('OpportunityType', 'Customer', 2),
  ('SMTP Authentication', 'None', 1),
  ('SMTP Authentication', 'SSL', 2),
  ('SMTP Authentication', 'TLS', 3),
  ('SMTP Encryption', 'None', 1),
  ('SMTP Encryption', 'Plain Password', 2),
  ('SMTP Encryption', 'Cram-MD5', 3),
  ('System Email Sender', 'FileMakar SMTP', 1),
  ('System Email Sender', 'MSGraph API', 2),
  ('SystemServerType', 'Development', 1),
  ('SystemServerType', 'Staging', 2),
  ('SystemServerType', 'Production', 3);
