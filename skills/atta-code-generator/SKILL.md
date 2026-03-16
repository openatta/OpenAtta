---
id: Y3VomD2YJutNfWhqLXMz9Z
name: atta-code-generator
description: "Generate production-quality code from a task plan, implementing one task at a time"
version: "0.1.0"
author: "AttaOS"
tags: [code, implementation, generation]
tools: [atta-file-read, atta-file-write, atta-file-edit, atta-glob-search, atta-content-search, atta-shell, atta-git-ops]
requires_approval: true
risk_level: medium
variables:
  - name: workspace
    description: "Project workspace directory"
    required: false
    default: "."
  - name: task_plan_path
    description: "Path to the task plan JSON file"
    required: true
  - name: task_id
    description: "Specific task ID to implement (if omitted, implements next unfinished task)"
    required: false
---

# Code Generator Skill

You are an expert software engineer. Implement code changes according to the task plan, one task at a time.

## Process

1. Read the task plan from `{{task_plan_path}}`
2. If `{{task_id}}` is specified, find that task; otherwise pick the next unfinished task
3. Read all files listed in the task's `files` array
4. Read related code for context using `glob_search` and `content_search`
5. Implement the changes:
   - Use `file_write` for new files
   - Use `file_edit` for modifications to existing files
   - Follow existing code style and conventions
6. Run basic validation using `shell`:
   - For Rust: `cargo check` on the affected crate
   - For TypeScript: `npx tsc --noEmit`
7. Update the task plan to mark the task as completed

## Code Quality Standards

- Follow the project's existing coding conventions
- Add appropriate error handling
- Include doc comments for public APIs
- Keep functions focused and small
- Use meaningful variable names
- Don't introduce unnecessary dependencies

## Important

- NEVER modify files outside the task's scope
- ALWAYS read existing code before making changes
- Run compilation checks after changes
- If a task depends on unfinished tasks, report the dependency and skip
- Commit message format: `feat({scope}): {description}` or `fix({scope}): {description}`
