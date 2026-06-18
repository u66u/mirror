import os
import re

TEST_FILES = [
    "src/backend/tests/exports.rs",
    "src/backend/tests/model_packs.rs",
    "src/backend/tests/media_worker.rs",
    "src/backend/tests/storage_integrity.rs",
    "src/backend/tests/jobs_queue.rs",
    "src/backend/tests/maintenance_cli.rs",
    "src/backend/tests/support/mod.rs",
    "src/backend/tests/assets_timeline.rs",
    "src/backend/tests/backups.rs",
    "src/backend/tests/semantic_index.rs",
    "src/backend/tests/upload_http.rs",
    "src/backend/tests/setup_owner.rs",
    "src/backend/tests/uploads.rs",
    "src/backend/tests/search.rs",
    "src/backend/tests/shares.rs",
    "src/backend/tests/device_token_security.rs",
    "src/backend/tests/worker_runtime.rs",
    "src/backend/tests/assets_promotion.rs",
    "src/backend/tests/session_security.rs",
]

def extract_string(s, start_idx):
    if s[start_idx] != '"': return None, start_idx
    escaped = False
    for i in range(start_idx + 1, len(s)):
        if escaped:
            escaped = False
            continue
        if s[i] == '\\':
            escaped = True
            continue
        if s[i] == '"':
            return s[start_idx:i+1], i+1
    return None, start_idx

def extract_balanced(s, start_idx, open_char, close_char):
    if s[start_idx] != open_char: return None, start_idx
    count = 1
    in_string = False
    escaped = False
    for i in range(start_idx + 1, len(s)):
        if in_string:
            if escaped:
                escaped = False
            elif s[i] == '\\':
                escaped = True
            elif s[i] == '"':
                in_string = False
            continue
        
        if s[i] == '"':
            in_string = True
            continue
            
        if s[i] == open_char:
            count += 1
        elif s[i] == close_char:
            count -= 1
            if count == 0:
                return s[start_idx:i+1], i+1
    return None, start_idx

for filepath in TEST_FILES:
    if not os.path.exists(filepath): continue
    
    with open(filepath, 'r') as f:
        content = f.read()

    # Need a sophisticated replacement because rustfmt splits lines.
    # Approach:
    # 1. replace sqlx::query_scalar::<_, Type>("...") with sqlx::query_scalar!("...")
    # 2. replace sqlx::query_scalar("...") with sqlx::query_scalar!("...")
    # 3. replace sqlx::query_as::<_, Type>("...") -> replace with sqlx::query!(...) - wait, query_as! takes record type. If Type is a tuple `(T1, T2)`, we have a problem.
    # If the file binds, we must extract `.bind(arg)` and insert `arg` into macro.
    
    print(f"Processing {filepath}...")
