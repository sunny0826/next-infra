import { useEffect, useState, type Dispatch, type SetStateAction } from "react";

/**
 * useState 的持久化变体：初始值从 localStorage 恢复，每次更新写回。
 * localStorage 不可用（隐私模式 / 序列化失败）时静默降级为纯内存态。
 */
export function usePersistedState<T>(
  storageKey: string,
  initialValue: T,
): [T, Dispatch<SetStateAction<T>>] {
  const [value, setValue] = useState<T>(() => {
    try {
      const raw = window.localStorage.getItem(storageKey);
      return raw === null ? initialValue : (JSON.parse(raw) as T);
    } catch {
      return initialValue;
    }
  });

  useEffect(() => {
    try {
      window.localStorage.setItem(storageKey, JSON.stringify(value));
    } catch {
      // localStorage 不可用时静默降级为内存态。
    }
  }, [storageKey, value]);

  return [value, setValue];
}
