-- Original Attow Network test data, MIT OR Apache-2.0.
-- This is a branch-model oracle fixture, not a physical SQL snapshot backend.
CREATE TEMP TABLE branch_model_expected (
    key BLOB PRIMARY KEY NOT NULL,
    value BLOB NOT NULL
) WITHOUT ROWID;
INSERT INTO branch_model_expected (key, value) VALUES (x'6130', x'0a');
INSERT INTO branch_model_expected (key, value) VALUES (x'6231', x'14');
