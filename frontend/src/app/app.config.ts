import {
  ApplicationConfig,
  inject,
  LOCALE_ID,
  provideAppInitializer,
  provideBrowserGlobalErrorListeners,
  provideZonelessChangeDetection,
} from '@angular/core';
import { DOCUMENT, registerLocaleData } from '@angular/common';
import localeZh from '@angular/common/locales/zh';
import { provideRouter } from '@angular/router';
import { Title } from '@angular/platform-browser';
import { provideAnimationsAsync } from '@angular/platform-browser/animations/async';
import { provideHttpClient, withInterceptors } from '@angular/common/http';
import { MAT_DIALOG_DEFAULT_OPTIONS, MatDialogConfig } from '@angular/material/dialog';
import { MAT_SNACK_BAR_DEFAULT_OPTIONS, MatSnackBarConfig } from '@angular/material/snack-bar';

import { routes } from './app.routes';
import { authInterceptor } from './core/auth.interceptor';
import { APP_CONFIG } from './core/runtime-config';
import { API_BASE_URL } from './core/runtime-config';
import { ApiConfiguration } from './generated/api/api-configuration';

registerLocaleData(localeZh);

function dialogDefaults(): MatDialogConfig {
  return Object.assign(new MatDialogConfig(), {
    maxWidth: 'calc(100vw - 32px)',
    maxHeight: 'calc(100dvh - 32px)',
  });
}

function snackBarDefaults(): MatSnackBarConfig {
  return Object.assign(new MatSnackBarConfig(), {
    horizontalPosition: 'center',
    verticalPosition: 'top',
    panelClass: ['app-snackbar'],
  });
}

function initializeRuntimeProduct(): void {
  const runtimeConfig = inject(APP_CONFIG);
  const document = inject(DOCUMENT);
  inject(Title).setTitle(runtimeConfig.appName);
  document.documentElement.dataset['appSlug'] = runtimeConfig.appSlug;
}

function generatedApiConfiguration(): ApiConfiguration {
  const configuration = new ApiConfiguration();
  configuration.rootUrl = inject(API_BASE_URL);
  return configuration;
}

export const appConfig: ApplicationConfig = {
  providers: [
    provideZonelessChangeDetection(),
    provideBrowserGlobalErrorListeners(),
    provideAppInitializer(initializeRuntimeProduct),
    provideAnimationsAsync(),
    provideHttpClient(withInterceptors([authInterceptor])),
    { provide: ApiConfiguration, useFactory: generatedApiConfiguration },
    provideRouter(routes),
    { provide: LOCALE_ID, useValue: 'zh-CN' },
    {
      provide: MAT_DIALOG_DEFAULT_OPTIONS,
      useFactory: dialogDefaults,
    },
    {
      provide: MAT_SNACK_BAR_DEFAULT_OPTIONS,
      useFactory: snackBarDefaults,
    },
  ],
};
