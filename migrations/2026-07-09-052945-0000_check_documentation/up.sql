-- Operator-authored documentation for a catalogued (source, check): a
-- single markdown document. Convention (seeded by the UI editor, not
-- enforced): what the check observes, what each result means, and hints
-- for solving a failure.
ALTER TABLE check_policies ADD COLUMN documentation TEXT;
