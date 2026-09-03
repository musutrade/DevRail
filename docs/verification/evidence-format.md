# DevRail 验证证据格式

版本：1

验证证据分为两层：

- 版本控制内的摘要索引：保存追踪 ID、UTC 时间、源码基线 SHA、变更范围、完整命令、结果和稳定 artifact 地址；
- CI artifact：保存详细脱敏日志、测试结果和扫描产物。artifact 不得包含密码、token、Cookie、私钥、数据库连接串、完整请求头或未脱敏绝对路径。

摘要文件使用 JSON，必须包含以下字段：

```json
{
  "schemaVersion": 1,
  "change": "OpenSpec change 名称",
  "sourceBaseSha": "40 位提交 SHA",
  "deliverySha": null,
  "generatedAtUtc": "ISO-8601 UTC 时间",
  "scope": ["backend", "frontend", "workflow"],
  "checks": [
    {
      "command": "完整命令",
      "status": "passed",
      "summary": "不含敏感值的结果摘要",
      "artifactUrl": null
    }
  ]
}
```

`deliverySha` 在本地实现阶段保持 `null`；只有提交产生后才能填写真实 SHA。
`artifactUrl` 在远端 CI 完成前保持 `null`，不得填写不可访问的临时路径。
本机 `codex-audit-pipeline/.codex/reports/` 仅用于调试，不得成为文档中“已通过”
声明的唯一依据。
