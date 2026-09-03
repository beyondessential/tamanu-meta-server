-- A restore is a backup concern and backups are the box's, so the window that
-- gates one is declared over a machine.
alter table machines
	add column restore_allowed_until timestamptz,
	add column restore_allowed_by text;

-- Every application sits on its own machine today, so this is a copy. Where a
-- box carries several, the latest window wins: the box is restorable for as
-- long as any window opened over it runs.
update machines m
set restore_allowed_until = w.allowed_until,
    restore_allowed_by = w.allowed_by
from (
	select distinct on (machine_id)
		machine_id,
		restore_allowed_until as allowed_until,
		restore_allowed_by as allowed_by
	from applications
	where machine_id is not null
	  and restore_allowed_until is not null
	order by machine_id, restore_allowed_until desc
) w
where m.id = w.machine_id;

alter table applications
	drop column restore_allowed_until,
	drop column restore_allowed_by;
