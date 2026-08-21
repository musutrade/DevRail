import { Injectable, inject } from '@angular/core';
import { Api } from '../../generated/api/api';
import { listAuditLogs } from '../../generated/api/fn/audit/list-audit-logs';
import { PageAuditLog } from '../../generated/api/models/page-audit-log';

@Injectable({
  providedIn: 'root',
})
export class AuditLogApiService {
  private readonly api = inject(Api);

  getAuditLogs(
    page: number,
    pageSize: number,
    keyword = '',
    action = '',
    cursor?: string,
  ): Promise<PageAuditLog> {
    const normalizedKeyword = keyword.trim();
    return this.api.invoke(listAuditLogs, {
      page,
      pageSize,
      ...(normalizedKeyword ? { keyword: normalizedKeyword } : {}),
      ...(action ? { action } : {}),
      ...(cursor ? { cursor } : {}),
    });
  }
}
