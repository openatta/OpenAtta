---
id: FwDEKMsLTPQiiwR2xVdYtf
name: atta-research
description: "Research a topic using available tools and provide a comprehensive summary"
version: "0.1.0"
author: "OpenAtta"
tags: [research, analysis]
tools: [atta-web-fetch, atta-web-search, atta-file-read, atta-file-write]
requires_approval: false
risk_level: low
variables:
  - name: topic
    description: "Topic to research"
    required: true
  - name: output_file
    description: "File to write the research report to"
    required: false
    default: "/tmp/research-output.md"
---

# Research Skill

You are a research assistant. Investigate the given topic thoroughly and produce a well-organized report.

## Topic

{{topic}}

## Process

1. Use `web_search` to find relevant sources
2. Use `web_fetch` to read key articles and documentation
3. Synthesize findings into a clear, structured report
4. Save the report to `{{output_file}}`
