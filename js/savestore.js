/* ============================================================
   STARFORGE - savestore.js
   存档存储层：IndexedDB 封装（多槽位 + 索引单事务原子写）
   取代 localStorage（配额约 5MB）→ IDB 配额数百 MB 起，异步不卡主线程
   IDB 不可用时降级为内存 Map（本会话有效），绝不触碰旧 localStorage 数据
   ============================================================ */
'use strict';

const SaveStore = (() => {
  const DB_NAME = 'starforge';
  const DB_VER = 1;
  const STORE = 'saves';
  const INDEX_KEY = '__index';       // 索引数组存放的保留键
  const MIGRATED_KEY = '__migrated'; // 旧 localStorage 迁移完成标记（保留键不与 starforge_sv_* 冲突）

  let dbPromise = null;   // open() 幂等：缓存打开中/已打开的 Promise
  let mem = null;         // 降级模式：内存 Map（IDB 打开失败时启用）
  let opened = false;     // open() 是否已 settle（available 在 open 前不应误报 true）

  function open(){
    if (dbPromise) return dbPromise;
    dbPromise = new Promise(resolve => {
      try {
        const req = indexedDB.open(DB_NAME, DB_VER);
        req.onupgradeneeded = () => {
          const db = req.result;
          if (!db.objectStoreNames.contains(STORE)) db.createObjectStore(STORE);
        };
        req.onsuccess = () => { opened = true; resolve(req.result); };
        req.onerror = () => { opened = true; mem = new Map(); resolve(null); };
        req.onblocked = () => { opened = true; mem = new Map(); resolve(null); };
      } catch(e){ opened = true; mem = new Map(); resolve(null); }
    });
    return dbPromise;
  }

  // 通用请求封装：成功 resolve(结果)，失败 resolve(null)（永不 reject，防全局异步错误浮窗）
  function req(request, okVal){
    return new Promise(resolve => {
      try {
        request.onsuccess = () => resolve(okVal !== undefined ? okVal : request.result);
        request.onerror = () => resolve(null);
        request.onabort = () => resolve(null);
      } catch(e){ resolve(null); }
    });
  }

  async function getSlot(key){
    await open();
    if (mem) return mem.has(key) ? JSON.parse(JSON.stringify(mem.get(key))) : null;
    try {
      const db = await dbPromise;
      if (!db) return null;
      const r = await req(db.transaction(STORE).objectStore(STORE).get(key));
      return r === undefined ? null : r;
    } catch(e){ return null; }
  }
  async function putSlot(key, data){
    await open();
    if (mem){ mem.set(key, JSON.parse(JSON.stringify(data))); return true; }
    try {
      const db = await dbPromise;
      if (!db) return false;
      return await new Promise(resolve => {
        let txn;
        try {
          txn = db.transaction(STORE, 'readwrite');
          txn.objectStore(STORE).put(data, key);
        } catch(e){ resolve(false); return; }
        txn.oncomplete = () => resolve(true);
        txn.onerror = () => resolve(false);
        txn.onabort = () => resolve(false);
      });
    } catch(e){ return false; }
  }
  async function deleteSlot(key){
    await open();
    if (mem){ mem.delete(key); return true; }
    try {
      const db = await dbPromise;
      if (!db) return false;
      return await new Promise(resolve => {
        let txn;
        try {
          txn = db.transaction(STORE, 'readwrite');
          txn.objectStore(STORE).delete(key);
        } catch(e){ resolve(false); return; }
        txn.oncomplete = () => resolve(true);
        txn.onerror = () => resolve(false);
        txn.onabort = () => resolve(false);
      });
    } catch(e){ return false; }
  }
  async function getIndex(){
    await open();
    if (mem) return mem.has(INDEX_KEY) ? JSON.parse(JSON.stringify(mem.get(INDEX_KEY))) : [];
    const r = await getSlot(INDEX_KEY);
    return Array.isArray(r) ? r : [];
  }
  function putIndex(arr){
    return putSlot(INDEX_KEY, arr);
  }
  // 槽位 + 索引在单个读写事务中写入：要么都成功要么都回滚（撕裂写不出一半的存档）
  async function atomicWrite(key, data, idx, idxKey){
    await open();
    const ik = idxKey || INDEX_KEY;
    if (mem){
      mem.set(key, JSON.parse(JSON.stringify(data)));
      mem.set(ik, JSON.parse(JSON.stringify(idx)));
      return true;
    }
    try {
      const db = await dbPromise;
      if (!db) return false;
      return await new Promise(resolve => {
        let txn;
        try {
          txn = db.transaction(STORE, 'readwrite');
          const st = txn.objectStore(STORE);
          st.put(data, key);
          st.put(idx, ik);
        } catch(e){ resolve(false); return; }
        txn.oncomplete = () => resolve(true);
        txn.onerror = () => resolve(false);
        txn.onabort = () => resolve(false);
      });
    } catch(e){ return false; }
  }
  async function isMigrated(){
    await open();
    const r = await getSlot(MIGRATED_KEY);   // 降级模式读内存 Map，与 setMigrated 一致
    return r === 1;
  }
  function setMigrated(){
    return putSlot(MIGRATED_KEY, 1);
  }

  return {
    open, getSlot, putSlot, deleteSlot, getIndex, putIndex, atomicWrite, isMigrated, setMigrated,
    get available(){ return opened && mem === null; },
  };
})();
window.SaveStore = SaveStore;
