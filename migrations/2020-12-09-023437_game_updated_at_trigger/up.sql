CREATE OR REPLACE FUNCTION update_game_updated_at_from_slot_proc()
RETURNS TRIGGER AS $$
BEGIN
UPDATE game SET updated_at = now() WHERE id = NEW."game_id";
RETURN NEW;
END;
$$ language 'plpgsql';

DROP TRIGGER IF EXISTS update_game_updated_at_slot ON game_used_slot;
CREATE TRIGGER update_game_updated_at_slot BEFORE UPDATE ON game_used_slot FOR EACH ROW EXECUTE PROCEDURE update_game_updated_at_from_slot_proc();