alter table applications
	add column restore_allowed_until timestamptz,
	add column restore_allowed_by text;

-- A box's window covers every workload on it, so going back gives each
-- application the window its machine held.
update applications a
set restore_allowed_until = m.restore_allowed_until,
    restore_allowed_by = m.restore_allowed_by
from machines m
where a.machine_id = m.id
  and m.restore_allowed_until is not null;

alter table machines
	drop column restore_allowed_until,
	drop column restore_allowed_by;
