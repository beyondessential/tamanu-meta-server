-- Let an application hold a group of its own again. Existing values are left
-- as they stand: they agree with their machine's, which is a valid state
-- either way.

DROP TRIGGER machine_group_propagates ON machines;
DROP FUNCTION machine_group_propagates();
DROP TRIGGER applications_take_machine_group ON applications;
DROP FUNCTION applications_take_machine_group();
