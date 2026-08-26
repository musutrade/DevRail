import { provideZonelessChangeDetection } from '@angular/core';
import { ComponentFixture, TestBed } from '@angular/core/testing';
import { MatSnackBar } from '@angular/material/snack-bar';
import { ActivatedRoute, provideRouter } from '@angular/router';
import { vi } from 'vitest';
import { DevRailApiService } from '../../features/devrail/data-access/devrail-api.service';
import type { DevRailTaskResponse } from '../../generated/api/models';
import { DevRailTaskDetailPage } from './devrail-task-detail';

const TASK: DevRailTaskResponse = {
  id: 7,
  projectId: 3,
  organizationId: 2,
  ownerUserId: 5,
  title: '验证工作流快照',
  goal: '确认排队任务使用不可变工作流',
  labels: [],
  priority: 'high',
  status: 'queued',
  revision: 4,
  schedulerAttempt: 1,
  schedulerMaxAttempts: 3,
  schedulerRetryCount: 0,
  workflowSource: 'repository',
  workflowVersion: '1',
  workflowDigest: 'b'.repeat(64),
  createdAt: '2026-08-26T00:00:00Z',
  updatedAt: '2026-08-26T00:01:00Z',
};

describe('DevRailTaskDetailPage', () => {
  let fixture: ComponentFixture<DevRailTaskDetailPage>;

  beforeEach(async () => {
    const apiStub: Partial<DevRailApiService> = {
      getTask: vi.fn(async () => TASK),
      listRepositories: vi.fn(async () => ({ items: [], total: 0, page: 1, pageSize: 20 })),
      listEnvironments: vi.fn(async () => ({ items: [], total: 0, page: 1, pageSize: 20 })),
      listTaskComments: vi.fn(async () => ({ items: [], total: 0, page: 1, pageSize: 50 })),
    };

    await TestBed.configureTestingModule({
      imports: [DevRailTaskDetailPage],
      providers: [
        provideZonelessChangeDetection(),
        provideRouter([]),
        {
          provide: ActivatedRoute,
          useValue: {
            snapshot: {
              paramMap: { get: (name: string) => (name === 'projectId' ? '3' : '7') },
            },
          },
        },
        { provide: DevRailApiService, useValue: apiStub },
        { provide: MatSnackBar, useValue: { open: vi.fn() } },
      ],
    }).compileComponents();

    fixture = TestBed.createComponent(DevRailTaskDetailPage);
    await vi.waitFor(() => expect(fixture.componentInstance.loading()).toBe(false));
    await fixture.whenStable();
  });

  it('显示任务修订号和工作流身份', () => {
    const text = fixture.nativeElement.textContent as string;
    expect(text).toContain('派发契约');
    expect(text).toContain('任务修订号：4');
    expect(text).toContain('工作流来源：repository');
    expect(text).toContain('工作流版本：1');
    expect(text).toContain(`工作流摘要：${'b'.repeat(64)}`);
  });
});
