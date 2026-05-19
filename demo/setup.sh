#!/bin/bash
# Setup demo workspace for vhs recording
set -e
DEMO=$(mktemp -d)
cd "$DEMO"

cat > config.json << 'ENDJSON'
{
    "name": "myapp",
    "version": "1.0.0",
    "database": {
        "port": 3306
    }
}
ENDJSON

cat > config.yaml << 'ENDYAML'
# App configuration
app:
  version: "1.0.0"
  debug: false
ENDYAML

cat > README.md << 'ENDMD'
# MyApp

Current version: 1.0.0

## Features

| Feature | Status |
|---------|--------|
| Auth    | Done   |
ENDMD

# Batch operations file
cat > ops.txt << 'ENDOPS'
doc.set config.json version "2.0.0"
doc.set config.yaml app.version "2.0.0"
replace README.md "1.0.0" "2.0.0"
md.table_append README.md "## Features" "| Search | Done |"
ENDOPS

echo "Demo workspace: $DEMO"
# Print the path so the tape can cd to it
echo "$DEMO" > /tmp/patchloom-demo-dir
