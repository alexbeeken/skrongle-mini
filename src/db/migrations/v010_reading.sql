-- Reading settings (#233): text size, spacing, letters, contrast and
-- movement, chosen on Profile and written on every page's <html> element.
-- One small text column parsed in one place (`reading.rs`); '' is all
-- standard, so every existing account keeps the look it has.
ALTER TABLE users ADD COLUMN reading TEXT NOT NULL DEFAULT '';
