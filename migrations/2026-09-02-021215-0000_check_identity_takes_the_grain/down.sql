-- Going back merges any two rows that share a source and a name, keeping the
-- one an operator most recently touched. Nothing writes a colliding pair today
-- (the grain is still a function of the name), so in practice this deletes
-- nothing.
delete from check_policies a
using check_policies b
where a.source = b.source
  and a.check_name = b.check_name
  and a.subject <> b.subject
  and (b.updated_at, b.subject) > (a.updated_at, a.subject);

alter table check_policies
	drop constraint check_policies_pkey,
	add constraint check_policies_pkey primary key (source, check_name);

alter table check_policies
	drop constraint check_policies_subject_known,
	drop column subject;
