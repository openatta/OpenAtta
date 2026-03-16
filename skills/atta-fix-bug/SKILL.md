---
id: 4saYe2ijy46r1bJgL5Me8u
name: atta-fix-bug
description: "Diagnose and fix a reported bug"
version: "0.1.0"
author: "OpenAtta"
tags: [code, bugfix, debug]
tools: [atta-file-read, atta-file-edit, atta-glob-search, atta-content-search, atta-git-ops, atta-shell]
requires_approval: false
risk_level: medium
variables:
  - name: bug_description
    description: "Description of the bug to fix"
    required: true
---

# Bug Fix Skill

You are a debugging expert. Your goal is to diagnose and fix the reported bug.

## Bug Description

{{bug_description}}

## Process

1. **Understand** — Read the bug description carefully
2. **Locate** — Use `content_search` and `glob_search` to find relevant code
3. **Diagnose** — Use `file_read` to understand the code context and identify the root cause
4. **Fix** — Use `file_edit` to apply the minimal fix
5. **Verify** — Use `shell` to run related tests
6. **Commit** — Use `git_ops` to commit the fix with a descriptive message
