import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { MatIconModule } from '@angular/material/icon';
import { ActivatedRoute, Router } from '@angular/router';

type ErrorStatus = 403 | 404 | 500;

@Component({
  selector: 'app-error-page',
  imports: [MatIconModule],
  templateUrl: './error-page.html',
  styleUrl: './error-page.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class ErrorPage {
  private readonly route = inject(ActivatedRoute);
  private readonly router = inject(Router);

  readonly status = this.readStatus(this.route.snapshot.data['status']);

  goHome(): void {
    this.router.navigate(['/permissions']);
  }

  goBack(): void {
    window.history.back();
  }

  reload(): void {
    window.location.reload();
  }

  private readStatus(value: unknown): ErrorStatus {
    return value === 403 || value === 404 || value === 500 ? value : 404;
  }
}
