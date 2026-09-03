-- There is no default type. A report is the only thing that creates an
-- application and it carries the type, so an application without one does not
-- arise — and a row that somehow lacked one would be better refused than
-- silently recorded as a Tamanu central it is not.
ALTER TABLE applications ALTER COLUMN type DROP DEFAULT;
