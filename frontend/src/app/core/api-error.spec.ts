import { HttpErrorResponse, HttpHeaders } from '@angular/common/http';
import { apiErrorMessage } from './api-error';

describe('apiErrorMessage', () => {
  it('adds the server trace id to internal errors', () => {
    const error = new HttpErrorResponse({
      status: 500,
      error: {
        error: {
          message: '服务器内部错误',
          traceId: 'trace-500',
        },
      },
    });

    expect(apiErrorMessage(error, '请求失败')).toBe(
      '服务器内部错误，请稍后重试（问题编号：trace-500）',
    );
  });

  it('uses the response header when an upstream error body has no trace id', () => {
    const error = new HttpErrorResponse({
      status: 502,
      error: { error: { message: '上游服务异常' } },
      headers: new HttpHeaders({ 'x-request-id': 'gateway-trace-1' }),
    });

    expect(apiErrorMessage(error, '请求失败')).toBe(
      '服务器内部错误，请稍后重试（问题编号：gateway-trace-1）',
    );
  });

  it('does not expose malformed trace ids or add ids to client errors', () => {
    const serverError = new HttpErrorResponse({
      status: 500,
      error: { error: { message: '服务器内部错误', traceId: 'unsafe trace id' } },
    });
    const validationError = new HttpErrorResponse({
      status: 422,
      error: { error: { message: '参数错误', traceId: 'trace-422' } },
    });

    expect(apiErrorMessage(serverError, '请求失败')).toBe('服务器内部错误，请稍后重试');
    expect(apiErrorMessage(validationError, '请求失败')).toBe('参数错误');
  });

  it('uses the fallback for non-HTTP errors', () => {
    expect(apiErrorMessage(new Error('internal detail'), '请求失败')).toBe('请求失败');
  });
});
