-- Add operational component version to tracked media nodes.
ALTER TABLE media_nodes ADD COLUMN version TEXT;
