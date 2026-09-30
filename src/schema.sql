PRAGMA journal_mode=WAL;
PRAGMA synchronous=FULL;
PRAGMA foreign_keys=ON;
CREATE TABLE IF NOT EXISTS meta(name TEXT PRIMARY KEY,value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS keys(
  id TEXT PRIMARY KEY, masked TEXT NOT NULL, active INTEGER NOT NULL, policy TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS models(id TEXT PRIMARY KEY);
CREATE TABLE IF NOT EXISTS prices(model TEXT PRIMARY KEY,price TEXT NOT NULL,revision INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS price_origins(
  model TEXT PRIMARY KEY REFERENCES prices(model) ON DELETE CASCADE,
  source TEXT NOT NULL, source_model TEXT NOT NULL, synced_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS requests(
  request_id TEXT NOT NULL, model TEXT NOT NULL, key_id TEXT NOT NULL,
  requested TEXT NOT NULL, trace_id TEXT NOT NULL, started INTEGER NOT NULL,
  completed INTEGER, outcome TEXT, status TEXT NOT NULL, price TEXT,
  PRIMARY KEY(request_id,model)
);
CREATE INDEX IF NOT EXISTS requests_trace ON requests(trace_id,model,key_id);
CREATE INDEX IF NOT EXISTS requests_review ON requests(key_id,model,status);
CREATE TABLE IF NOT EXISTS usage(
  id TEXT PRIMARY KEY,request_id TEXT NOT NULL,key_id TEXT NOT NULL,model TEXT NOT NULL,
  started INTEGER NOT NULL,tokens INTEGER NOT NULL,cost INTEGER,detail TEXT NOT NULL,
  failed INTEGER NOT NULL,stream INTEGER NOT NULL,manual INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS usage_period ON usage(key_id,model,started);
CREATE INDEX IF NOT EXISTS usage_key_period ON usage(key_id,started);
CREATE INDEX IF NOT EXISTS usage_request ON usage(request_id,model);
CREATE TABLE IF NOT EXISTS audit(
  id INTEGER PRIMARY KEY,created INTEGER NOT NULL,action TEXT NOT NULL,subject TEXT NOT NULL,detail TEXT NOT NULL
);
