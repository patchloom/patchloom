#!/bin/bash
# Setup demo workspace for vhs recording
set -e
DEMO=$(mktemp -d)
cd "$DEMO"

cat > package.json << 'ENDJSON'
{
  "name": "myapp",
  "version": "1.0.0",
  "description": "My application"
}
ENDJSON

cat > config.yaml << 'ENDYAML'
# Application configuration
app:
  name: myapp
  version: 1.0.0

# Database settings
database:
  host: localhost
  port: 5432  # PostgreSQL default
  pool_size: 10  # max connections
ENDYAML

cat > settings.toml << 'ENDTOML'
# Project metadata
[project]
name = "myapp"
# Follows semver
version = "1.0.0"
ENDTOML

cat > README.md << 'ENDMD'
# MyApp

Current version: 1.0.0

## Features

| Feature | Status |
|---------|--------|
| Auth    | Done   |
ENDMD

# Batch operations: 6 edits across 4 files in 3 formats
cat > ops.txt << 'ENDOPS'
doc.set package.json version "2.0.0"
doc.set config.yaml app.version "2.0.0"
doc.set config.yaml database.port 5433
doc.set settings.toml project.version "2.0.0"
replace README.md "1.0.0" "2.0.0"
md.table_append README.md "## Features" "| Search | Done |"
ENDOPS

echo "Demo workspace: $DEMO"
# Print the path so the tape can cd to it
echo "$DEMO" > /tmp/patchloom-demo-dir
