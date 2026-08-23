import { DatePipe } from '@angular/common';
import {
  ChangeDetectionStrategy,
  Component,
  OnDestroy,
  OnInit,
  inject,
  signal,
} from '@angular/core';
import { ActivatedRoute, RouterLink } from '@angular/router';
import { MatIconModule } from '@angular/material/icon';
import { MatProgressSpinnerModule } from '@angular/material/progress-spinner';
import { MatSnackBar, MatSnackBarModule } from '@angular/material/snack-bar';
import { apiErrorMessage } from '../../core/api-error';
import { DevRailApiService } from '../../features/devrail/data-access/devrail-api.service';
import type { DevRailRunEventResponse, DevRailRunResponse } from '../../generated/api/models';

@Component({
  selector: 'app-devrail-run',
  imports: [DatePipe, MatIconModule, MatProgressSpinnerModule, MatSnackBarModule, RouterLink],
  templateUrl: './devrail-run.html',
  styleUrl: './devrail-run.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class DevRailRunPage implements OnInit, OnDestroy {
  readonly run = signal<DevRailRunResponse | null>(null);
  readonly events = signal<DevRailRunEventResponse[]>([]);
  readonly loading = signal(true);
  readonly busy = signal(false);
  readonly error = signal<string | null>(null);
  private readonly route = inject(ActivatedRoute);
  private readonly api = inject(DevRailApiService);
  private readonly snack = inject(MatSnackBar);
  private eventSource?: EventSource;
  private runId = 0;

  ngOnInit(): void {
    this.runId = Number(this.route.snapshot.paramMap.get('id'));
    void this.load();
  }

  ngOnDestroy(): void {
    this.eventSource?.close();
  }

  async interrupt(): Promise<void> {
    if (!this.run() || this.busy()) return;
    this.busy.set(true);
    try {
      this.run.set(await this.api.interruptRun(this.runId));
      this.snack.open('运行已请求中断', '关闭', { duration: 2500 });
    } catch (error) {
      this.snack.open(apiErrorMessage(error, '中断运行失败'), '关闭', { duration: 5000 });
    } finally {
      this.busy.set(false);
    }
  }

  async retry(): Promise<void> {
    await this.performRetry(false);
  }

  async retryFromTurn(): Promise<void> {
    await this.performRetry(true);
  }

  private async performRetry(resume: boolean): Promise<void> {
    const current = this.run();
    if (!current || this.busy()) return;
    this.busy.set(true);
    try {
      const next = await this.api.retryRun(this.runId, {
        idempotencyKey: `ui-retry-${Date.now()}`,
        input: null,
        ...(resume && current.turnId ? { resumeFromTurnId: current.turnId } : {}),
      });
      this.snack.open(`已创建新的运行 #${next.id}`, '关闭', { duration: 3000 });
      this.runId = next.id;
      this.run.set(next);
      this.events.set([]);
      this.connectEvents();
    } catch (error) {
      this.snack.open(apiErrorMessage(error, '重试运行失败'), '关闭', { duration: 5000 });
    } finally {
      this.busy.set(false);
    }
  }

  eventPayload(event: DevRailRunEventResponse): string {
    return event.summary || JSON.stringify(event.payload);
  }

  private async load(): Promise<void> {
    this.loading.set(true);
    this.error.set(null);
    try {
      this.run.set(await this.api.getRun(this.runId));
      const page = await this.api.listRunEvents(this.runId);
      this.events.set(page.items);
      this.connectEvents();
    } catch (error) {
      this.error.set(apiErrorMessage(error, '运行加载失败'));
    } finally {
      this.loading.set(false);
    }
  }

  private connectEvents(): void {
    this.eventSource?.close();
    this.eventSource = new EventSource(`/api/v1/runs/${this.runId}/events/stream`, {
      withCredentials: true,
    });
    this.eventSource.onmessage = (message) => this.appendEvent(message.data);
    this.eventSource.addEventListener('run_started', (event) =>
      this.appendEvent((event as MessageEvent).data),
    );
    this.eventSource.addEventListener('turn_complete', (event) =>
      this.appendEvent((event as MessageEvent).data),
    );
    this.eventSource.onerror = () => {
      this.eventSource?.close();
      if (!['completed', 'failed', 'cancelled'].includes(this.run()?.status ?? '')) {
        window.setTimeout(() => this.connectEvents(), 1500);
      }
    };
  }

  private appendEvent(data: string): void {
    try {
      const event = JSON.parse(data) as DevRailRunEventResponse;
      this.events.update((items) =>
        items.some((item) => item.cursor === event.cursor) ? items : [...items, event],
      );
    } catch {
      // SSE payloads are server-generated JSON; malformed data is ignored safely.
    }
  }
}
