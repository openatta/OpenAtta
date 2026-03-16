-- Add token expiration support
ALTER TABLE remote_agents ADD COLUMN token_expires_at TIMESTAMPTZ;
