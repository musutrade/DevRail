import {
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  ElementRef,
  HostListener,
  inject,
  signal,
  viewChild,
} from '@angular/core';
import { takeUntilDestroyed } from '@angular/core/rxjs-interop';
import { NavigationEnd, Router, RouterLink, RouterLinkActive, RouterOutlet } from '@angular/router';
import { filter } from 'rxjs';
import { MatIconModule } from '@angular/material/icon';
import { MatDialog, MatDialogModule } from '@angular/material/dialog';
import { MatMenuModule } from '@angular/material/menu';
import { MatSnackBar, MatSnackBarModule } from '@angular/material/snack-bar';
import { ThemeService } from '../core/theme.service';
import { AuthService } from '../core/auth.service';
import { ChangePasswordDialog } from '../core/change-password.dialog';
import { APP_CONFIG } from '../core/runtime-config';
import { APP_NAVIGATION, NavigationGroup, NavigationItem } from '../app.navigation';

@Component({
  selector: 'app-layout',
  imports: [
    RouterOutlet,
    RouterLink,
    RouterLinkActive,
    MatIconModule,
    MatDialogModule,
    MatMenuModule,
    MatSnackBarModule,
  ],
  templateUrl: './layout.html',
  styleUrl: './layout.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class LayoutComponent {
  readonly collapsed = signal(false);
  readonly sidebarOpen = signal(false);
  readonly navigationItems = APP_NAVIGATION;
  readonly expandedGroups = signal<ReadonlySet<string>>(new Set());
  readonly currentUrl = signal('');
  private readonly sidebarPanel = viewChild.required<ElementRef<HTMLElement>>('sidebarPanel');
  private readonly mobileMenuButton =
    viewChild.required<ElementRef<HTMLButtonElement>>('mobileMenuButton');
  private readonly mainContent = viewChild.required<ElementRef<HTMLElement>>('mainContent');
  private readonly theme = inject(ThemeService);
  private readonly router = inject(Router);
  private readonly destroyRef = inject(DestroyRef);
  private readonly dialog = inject(MatDialog);
  private readonly snackBar = inject(MatSnackBar);
  readonly auth = inject(AuthService);
  readonly appConfig = inject(APP_CONFIG);
  readonly isDark = this.theme.isDark;

  constructor() {
    this.syncNavigation(this.router.url);
    this.router.events
      .pipe(
        filter((event): event is NavigationEnd => event instanceof NavigationEnd),
        takeUntilDestroyed(this.destroyRef),
      )
      .subscribe((event) => this.syncNavigation(event.urlAfterRedirects));
  }

  toggleTheme(): void {
    this.theme.toggle();
  }

  toggleSidebar(): void {
    this.collapsed.update((v) => !v);
  }

  canAccess(item: NavigationItem): boolean {
    return this.auth.hasAllPermissions(item.permissions);
  }

  isNavigationActive(item: NavigationItem): boolean {
    return item.kind === 'group'
      ? item.children.some((child) => this.isRouteActive(child.route))
      : this.isRouteActive(item.route);
  }

  isGroupExpanded(id: string): boolean {
    return this.expandedGroups().has(id);
  }

  toggleNavigationGroup(group: NavigationGroup): void {
    if (this.collapsed()) {
      this.collapsed.set(false);
    }
    this.expandedGroups.update((current) => {
      const next = new Set(current);
      if (next.has(group.id)) {
        next.delete(group.id);
      } else {
        next.add(group.id);
      }
      return next;
    });
  }

  toggleMobileNavigation(): void {
    if (this.sidebarOpen()) {
      this.closeMobileNavigation(true);
      return;
    }

    this.sidebarOpen.set(true);
    requestAnimationFrame(() => this.sidebarPanel().nativeElement.focus());
  }

  closeMobileNavigation(restoreFocus = false): void {
    this.sidebarOpen.set(false);
    if (restoreFocus) {
      requestAnimationFrame(() => this.mobileMenuButton().nativeElement.focus());
    }
  }

  focusMainContent(event: MouseEvent): void {
    event.preventDefault();
    this.mainContent().nativeElement.focus();
  }

  @HostListener('document:keydown.escape')
  closeMobileNavigationOnEscape(): void {
    if (this.sidebarOpen()) {
      this.closeMobileNavigation(true);
    }
  }

  openChangePassword(): void {
    this.dialog
      .open(ChangePasswordDialog)
      .afterClosed()
      .pipe(takeUntilDestroyed(this.destroyRef))
      .subscribe((changed: boolean | undefined) => {
        if (changed) {
          this.snackBar.open('密码修改成功，请重新登录', '关闭', { duration: 3000 });
          void this.router.navigate(['/login']);
        }
      });
  }

  async logout(): Promise<void> {
    try {
      await this.auth.logout();
    } catch {
      this.snackBar.open('退出请求未完成，请稍后重试', '关闭', { duration: 3000 });
    } finally {
      await this.router.navigate(['/login']);
    }
  }

  private syncNavigation(url: string): void {
    this.currentUrl.set(url);
    this.expandedGroups.update((current) => {
      const next = new Set(current);
      for (const item of this.navigationItems) {
        if (
          item.kind === 'group' &&
          item.children.some((child) => this.isRouteActive(child.route))
        ) {
          next.add(item.id);
        }
      }
      return next;
    });
  }

  private isRouteActive(route: string): boolean {
    const url = this.currentUrl();
    return url === route || url.startsWith(`${route}/`) || url.startsWith(`${route}?`);
  }
}
