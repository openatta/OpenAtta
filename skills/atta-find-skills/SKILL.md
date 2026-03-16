---
id: WcKNSThhoYAzHJsS7pbmU5
name: atta-find-skills
description: "Find and recommend available skills for a given task"
version: "0.1.0"
author: "OpenAtta"
tags: [meta, discovery]
tools: [atta-file-read, atta-glob-search]
requires_approval: false
risk_level: low
variables:
  - name: task_description
    description: "Description of what the user wants to accomplish"
    required: true
---

# Skill Discovery

You help users find the right skill for their task.

## User Request

{{task_description}}

## Process

1. Use `glob_search` to find all `SKILL.md` files in the skills directories
2. Use `file_read` to read each skill's description and capabilities
3. Recommend the most relevant skill(s) based on the user's task description
4. Explain why each recommendation is suitable
