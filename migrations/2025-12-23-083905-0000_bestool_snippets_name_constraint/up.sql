-- Add check constraint for valid snippet names
-- Names must:
-- 1. Not contain spaces
-- 2. Not contain: . / < > : " ' \ | ? * or control characters (U+0000 through U+001F)
-- 3. Not be Windows reserved filenames (case-insensitive)

ALTER TABLE bestool_snippets
ADD CONSTRAINT valid_snippet_name CHECK (
  -- No spaces
  name NOT LIKE '% %' AND
  -- No forbidden special characters: . / < > : " ' \ | ? *
  name NOT LIKE '%.%' AND
  name NOT LIKE '%/%' AND
  name NOT LIKE '%<%' AND
  name NOT LIKE '%>%' AND
  name NOT LIKE '%:%' AND
  name NOT LIKE '%"%' AND
  name NOT LIKE '%''%' AND
  name NOT LIKE '%\%' AND
  name NOT LIKE '%|%' AND
  name NOT LIKE '%?%' AND
  name NOT LIKE '%*%' AND
  -- No control characters (U+0000-U+001F) - match any character in range
  name !~ '[\x00-\x1f]' AND
  -- Not Windows reserved names (case-insensitive)
  LOWER(name) NOT IN (
    'con', 'prn', 'aux', 'nul',
    'com1', 'com2', 'com3', 'com4', 'com5', 'com6', 'com7', 'com8', 'com9',
    'lpt1', 'lpt2', 'lpt3', 'lpt4', 'lpt5', 'lpt6', 'lpt7', 'lpt8', 'lpt9'
  )
);
