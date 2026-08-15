/* ============================================================
   STARFORGE - modellib.js
   外部 GLB 模型库：base64 解码 → GLTFLoader 解析 → 带骨骼克隆
   （解析失败时调用方回退到程序化体素模型）

   与旧版区别：保留 SkinnedMesh + 骨骼 + 动画片段（clips），
   生物可用 AnimationMixer 播放 Walk / Idle / Death 等真实动画，
   四肢随行走自然摆动。
   ============================================================ */
'use strict';

const ModelLib = (() => {
  const ready = {};    // name -> { scene(蒙皮模板), clips, box(Box3), maxDim }
  let inited = false;

  function b64ToArr(b64){
    const s = atob(b64);
    const a = new Uint8Array(s.length);
    for (let i = 0; i < s.length; i++) a[i] = s.charCodeAt(i);
    return a.buffer;
  }

  function init(){
    if (inited) return;
    inited = true;
    if (typeof THREE === 'undefined' || typeof THREE.GLTFLoader !== 'function') return;
    if (typeof MODELS_B64 === 'undefined') return;
    const loader = new THREE.GLTFLoader();
    for (const name in MODELS_B64){
      try {
        loader.parse(b64ToArr(MODELS_B64[name]), '', gltf => {
          gltf.scene.updateMatrixWorld(true);
          const box = new THREE.Box3().setFromObject(gltf.scene);
          const size = new THREE.Vector3();
          box.getSize(size);
          const clips = Array.isArray(gltf.animations) ? gltf.animations : [];
          ready[name] = { scene: gltf.scene, clips, box, maxDim: Math.max(size.x, size.y, size.z) || 1 };
        }, e => { console.warn('[ModelLib]', name, e); });
      } catch(e){ console.warn('[ModelLib]', name, e); }
    }
  }

  // 带骨骼克隆：克隆整棵树（骨骼在场景层级内），再为每个 SkinnedMesh 重建 Skeleton。
  // 关键：骨架骨骼必须按【原骨架 bones 顺序】映射克隆（boneInverses 与 bones 一一对应）。
  // 场景遍历顺序 ≠ 蒙皮关节顺序（GLB 含 _end/Armature 等辅助骨骼且排序不同），
  // 顺序错位会导致顶点被错误逆绑定矩阵拉扯 → 模型严重扭曲。
  function cloneRigged(src){
    const g = src.clone(true);
    // 原始骨骼 → 克隆骨骼 一一映射（两棵树同构，先序遍历顺序一致）
    const srcBones = [], dstBones = [];
    src.traverse(o => { if (o.isBone) srcBones.push(o); });
    g.traverse(o => { if (o.isBone) dstBones.push(o); });
    const boneMap = new Map();
    for (let i = 0; i < srcBones.length && i < dstBones.length; i++) boneMap.set(srcBones[i], dstBones[i]);
    // 蒙皮网格：按原骨架顺序重建
    const srcMeshes = [], dstMeshes = [];
    src.traverse(o => { if (o.isSkinnedMesh) srcMeshes.push(o); });
    g.traverse(o => { if (o.isSkinnedMesh) dstMeshes.push(o); });
    for (let i = 0; i < srcMeshes.length && i < dstMeshes.length; i++){
      const sk0 = srcMeshes[i].skeleton;
      const sm = dstMeshes[i];
      if (sk0){
        sm.skeleton = new THREE.Skeleton(sk0.bones.map(b => boneMap.get(b) || b), sk0.boneInverses);
        sm.frustumCulled = false;   // 骨骼动画会超出静态包围球，关闭剔除防部件莫名消失
      }
    }
    return g;
  }

  function has(name){ init(); return !!ready[name]; }

  // 模板访问（测试/诊断用）：scene 含蒙皮网格与骨架，clips 为动画片段
  function getTemplate(name){ init(); return ready[name] ? { scene: ready[name].scene, clips: ready[name].clips } : null; }

  // 取模型克隆：
  //   size — 归一化最长边（世界单位）
  //   opts.tint — 整体向该颜色混合（生物按生态染色）
  //   opts.yaw  — 模型自带朝向修正（弧度），使“前方”=-Z
  //   opts.ground — true 时最低点落在 y=0（默认）；false 时几何中心在原点
  // 返回 wrap Group：userData.clips = 动画片段数组（可用 AnimationMixer 播放）
  function get(name, size, opts){
    init();
    const t = ready[name];
    if (!t) return null;
    opts = opts || {};
    const inner = cloneRigged(t.scene);
    // GLTF 的 StandardMaterial 统一转 Lambert（与全游戏光照风格一致，且修正线性色偏暗）
    const matCache = new Map();
    inner.traverse(o => {
      if ((o.isMesh || o.isSkinnedMesh) && o.material){
        if (!matCache.has(o.material)){
          const src = o.material;
          const m = new THREE.MeshLambertMaterial({
            color: src.color ? src.color.clone().convertLinearToSRGB() : new THREE.Color(0xaaaaaa),
            transparent: !!src.transparent,
            opacity: src.opacity !== undefined ? src.opacity : 1,
          });
          if (src.map){
            m.map = src.map;
            m.map.encoding = THREE.LinearEncoding;   // 避免 sRGB 双重解码变暗
          }
          if (src.vertexColors) m.vertexColors = true;
          if (o.isSkinnedMesh) m.skinning = true;    // r128 渲染器按 material.skinning 装配蒙皮管线
          if (opts.tint !== undefined) m.color.lerp(new THREE.Color(opts.tint), 0.55);
          matCache.set(o.material, m);
        }
        o.material = matCache.get(o.material);
      }
    });
    const s = size / t.maxDim;
    const wrap = new THREE.Group();
    const pivot = new THREE.Group();
    pivot.add(inner);
    pivot.rotation.y = opts.yaw || 0;
    // 先居中（x/z 及可选 y），再缩放
    const c = new THREE.Vector3();
    t.box.getCenter(c);
    inner.position.set(-c.x, opts.ground === false ? -c.y : -t.box.min.y, -c.z);
    pivot.scale.setScalar(s);
    wrap.add(pivot);
    wrap.userData.clips = t.clips;
    return wrap;
  }

  return { init, has, get, getTemplate };
})();
window.ModelLib = ModelLib;
ModelLib.init();   // 页面加载即解析（进入游戏前就绪；失败时调用方自动回退体素模型）
