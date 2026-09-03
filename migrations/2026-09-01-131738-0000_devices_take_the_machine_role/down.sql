-- Back to the name the role had when a box was a server. Lossless: `machine`
-- and `server` are the same role under two spellings.
UPDATE devices SET role = 'server' WHERE role = 'machine';
