//! Persistent frontier guild: renewable supply orders, authored expeditions,
//! field surveys and one-time milestones. Rewards use atomic inventory swaps.

use crate::{data, inventory::Inventory, player::Player};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};

#[derive(Clone, Debug)]
pub struct Contract {
    pub id: &'static str,
    pub title: &'static str,
    pub client: &'static str,
    pub desc: &'static str,
    pub rank: usize,
    pub tech: &'static str,
    pub cargo: &'static [(&'static str, i32)],
}

impl Contract {
    pub fn unlocked(&self, techs: &[String]) -> bool {
        data::tech_unlocked(techs, self.tech)
            && self.cargo.iter().all(|(item, _)| {
                let recipes: Vec<_> = data::RECIPES
                    .iter()
                    .filter(|r| r.output.0 == *item)
                    .collect();
                recipes.is_empty() || recipes.iter().any(|r| data::recipe_unlocked(techs, r))
            })
    }

    pub fn credits(&self) -> i32 {
        // A logistics premium over base market value; no arbitrary flat payouts
        // that make advanced manufactured goods less valuable than raw ore.
        80 + self
            .cargo
            .iter()
            .map(|(id, n)| data::item_by_key(id).map_or(0, |i| i.price) * n)
            .sum::<i32>()
            * 5
            / 4
    }
}

macro_rules! order {
    ($id:literal, $title:literal, $client:literal, $desc:literal, $rank:literal, $tech:literal, $(($item:literal, $n:literal)),+ $(,)?) => {
        Contract { id: $id, title: $title, client: $client, desc: $desc, rank: $rank, tech: $tech, cargo: &[$(($item, $n)),+] }
    };
}

pub const CONTRACTS: &[Contract] = &[
    order!(
        "air",
        "最后一罐空气",
        "晨曦救援队",
        "失压舱正在等待补给。保留自己的氧气，再交付救援物资。",
        0,
        "survival",
        ("oxygen", 20),
        ("sodium", 8)
    ),
    order!(
        "camp",
        "雨季前的营地",
        "漂泊者营地",
        "加固临时居所，让第一批移民熬过漫长的雨夜。",
        0,
        "survival",
        ("stone", 40),
        ("carbon", 20)
    ),
    order!(
        "ore",
        "第一炉火",
        "赤铜工坊",
        "工坊重新点火，急需两种矿石校准炉温。",
        0,
        "survival",
        ("iron_ore", 24),
        ("copper_ore", 16)
    ),
    order!(
        "fuel",
        "等待返航的人",
        "远航邮局",
        "邮船已经装满家书，只缺最后的发射燃料。",
        0,
        "survival",
        ("fuel", 2)
    ),
    order!(
        "carbon",
        "激光器的口粮",
        "地质勘测局",
        "地下勘探队的采矿激光即将耗尽能源。",
        0,
        "survival",
        ("carbon", 50),
        ("coal", 15)
    ),
    order!(
        "glass",
        "看得见星空的家",
        "穹顶建筑社",
        "新营地希望每个房间都有一扇窗。",
        0,
        "survival",
        ("glass_b", 12),
        ("planks_b", 16)
    ),
    order!(
        "gears",
        "重新转动",
        "赤铜工坊",
        "一批磨损的齿轮让运输线停摆。",
        1,
        "automation",
        ("gear", 12),
        ("iron", 16)
    ),
    order!(
        "wire",
        "接通灯火",
        "穹顶建筑社",
        "为避难所接入第一条稳定供电线路。",
        1,
        "power",
        ("wire", 18),
        ("circuit", 4)
    ),
    order!(
        "belts",
        "矿石不再靠背",
        "地质勘测局",
        "矿工申请一套短距离输送设备。",
        1,
        "automation",
        ("belt_b", 12),
        ("gear", 6)
    ),
    order!(
        "panels",
        "装甲补丁",
        "晨曦救援队",
        "救援艇需要轻量装甲修补微陨石造成的破口。",
        1,
        "assembly",
        ("plate", 8),
        ("wire", 8)
    ),
    order!(
        "beacon",
        "别让灯塔熄灭",
        "远航邮局",
        "为边疆邮路铺设路标和备用电路。",
        1,
        "logistics",
        ("beacon_b", 2),
        ("circuit", 4)
    ),
    order!(
        "metal",
        "新矿井的骨架",
        "地质勘测局",
        "深层井架需要成批金属，适合建立连续冶炼线。",
        1,
        "metallurgy",
        ("iron", 40),
        ("copper", 24)
    ),
    order!(
        "polymer",
        "密封工程",
        "穹顶建筑社",
        "舱室的弹性密封层需要耐用聚合物。",
        2,
        "materials",
        ("polymer", 16),
        ("glass_b", 12)
    ),
    order!(
        "coolant",
        "过热的实验室",
        "天穹研究院",
        "反应设备持续升温，请运来封装冷却液。",
        2,
        "fluidics",
        ("coolant", 6),
        ("fluid_canister", 4)
    ),
    order!(
        "acid",
        "晶圆清洗",
        "赤铜工坊",
        "下一代计算机需要洁净晶圆和化学清洗剂。",
        2,
        "fluidics",
        ("acid", 6),
        ("silicon_wafer", 12)
    ),
    order!(
        "battery",
        "漫长极夜",
        "漂泊者营地",
        "极夜期间太阳能停产，储能电芯将维持营地运转。",
        2,
        "energy_storage",
        ("battery_cell", 8),
        ("wire", 12)
    ),
    order!(
        "medicine",
        "移动诊疗站",
        "晨曦救援队",
        "偏远矿区的诊疗船正在补充无菌材料。",
        2,
        "biotech",
        ("medkit", 4),
        ("biofiber", 8)
    ),
    order!(
        "pipes",
        "第一口净水",
        "穹顶建筑社",
        "用密封管路把水从地下送到每一间舱室。",
        2,
        "fluidics",
        ("pipe_b", 20),
        ("tank_b", 1)
    ),
    order!(
        "logic",
        "自动分拣中心",
        "远航邮局",
        "升级邮件和工业货物的自动分拣系统。",
        3,
        "advanced_logistics",
        ("advanced_circuit", 6),
        ("filter_b", 2)
    ),
    order!(
        "alloy",
        "穿过碎石带",
        "晨曦救援队",
        "旗舰准备执行危险救援，需要强化船壳。",
        3,
        "ship_systems",
        ("ship_alloy", 12),
        ("plate", 8)
    ),
    order!(
        "super",
        "低温计算阵列",
        "天穹研究院",
        "扩建恒星观测阵列的超导信号系统。",
        3,
        "energy_storage",
        ("superconductor", 8),
        ("circuit", 10)
    ),
    order!(
        "filter",
        "呼吸权",
        "漂泊者营地",
        "剧毒星球上的孩子们需要新的空气过滤器。",
        3,
        "environmental",
        ("filter_core", 6),
        ("oxygen_cell", 6)
    ),
    order!(
        "thermal",
        "熔岩边的钻探",
        "地质勘测局",
        "热井设备需要抵御熔岩附近的持续高温。",
        3,
        "geothermal",
        ("heat_alloy", 16),
        ("pump_b", 2)
    ),
    order!(
        "defense",
        "守住撤离区",
        "晨曦救援队",
        "撤离点需要自动防御和备用供电。",
        3,
        "combat",
        ("turret_b", 1),
        ("battery_cell", 6)
    ),
    order!(
        "colony",
        "千人定居计划",
        "穹顶建筑社",
        "向新殖民地运送生命保障和医疗物资。",
        4,
        "colonization",
        ("oxygen_cell", 12),
        ("medkit", 8),
        ("biofiber", 16)
    ),
    order!(
        "warp",
        "通向下一片星海",
        "天穹研究院",
        "远征舰队即将突破已知航道。",
        4,
        "warp",
        ("warpcell", 1),
        ("fuel", 4)
    ),
    order!(
        "reactor",
        "重启恒星之心",
        "赤铜工坊",
        "一座大型工厂需要新的裂变材料与散热组件。",
        4,
        "nuclear",
        ("uranium", 20),
        ("coolant", 8),
        ("heat_alloy", 10)
    ),
    order!(
        "suit",
        "极地救援装备",
        "晨曦救援队",
        "为失事船搜索队配齐保温和呼吸设备。",
        4,
        "exosuit",
        ("cryo_module", 1),
        ("oxygen_tank", 1),
        ("medkit", 4)
    ),
    order!(
        "core",
        "点亮无人前哨",
        "漂泊者营地",
        "前哨站的无人控制中枢等待最后一批精密部件。",
        4,
        "advanced_logistics",
        ("advanced_circuit", 12),
        ("superconductor", 8),
        ("battery_cell", 12)
    ),
    order!(
        "antimatter",
        "深空能源储备",
        "天穹研究院",
        "研究站需要安全封装的反物质与舰用合金。",
        4,
        "nuclear",
        ("antimatter", 2),
        ("ship_alloy", 8)
    ),
];

#[derive(Clone, Copy, Debug)]
pub enum Goal {
    Deliver(&'static [(&'static str, i32)]),
    Event(&'static str, u32),
    Build(&'static str, u32),
    Survey(u32),
    Research(&'static str),
}

#[derive(Clone, Copy, Debug)]
pub struct Step {
    pub title: &'static str,
    pub story: &'static str,
    pub goal: Goal,
}

pub struct Expedition {
    pub id: &'static str,
    pub name: &'static str,
    pub desc: &'static str,
    pub rank: usize,
    pub steps: &'static [Step],
}

macro_rules! step {
    ($title:literal, $story:literal, $goal:expr) => {
        Step {
            title: $title,
            story: $story,
            goal: $goal,
        }
    };
}

pub const EXPEDITIONS: &[Expedition] = &[
    Expedition {
        id: "shelter",
        name: "长夜里的灯",
        desc: "从紧急补给开始，为失联的拓荒者建立一条归途。",
        rank: 0,
        steps: &[
            step!(
                "应急储备",
                "交付补给箱；请先为自己的生命维持系统留足资源。",
                Goal::Deliver(&[("oxygen", 12), ("carbon", 24)])
            ),
            step!(
                "广播塔",
                "本阶段开始后放置一座信标，让迷路的人找到营地。",
                Goal::Build("beacon", 1)
            ),
            step!(
                "记录家园",
                "完成任意一种生态的调查：三处相距至少 64m 的地面扫描，再提交样本。",
                Goal::Survey(1)
            ),
            step!(
                "留下火种",
                "送出最后一批建筑物资。频段里终于传来回应：我们看见灯了。",
                Goal::Deliver(&[("glass_b", 8), ("planks_b", 20), ("fuel", 1)])
            ),
        ],
    },
    Expedition {
        id: "foundry",
        name: "沉默的流水线",
        desc: "让一座停摆的边疆工坊重新运转。",
        rank: 0,
        steps: &[
            step!(
                "重新点火",
                "掌握冶金学，找回失落的工业基础。已有研究也有效。",
                Goal::Research("metallurgy")
            ),
            step!(
                "钢铁骨架",
                "交付成批金属，为修复工作建立物料基础。",
                Goal::Deliver(&[("iron", 24), ("copper", 12)])
            ),
            step!(
                "生产动脉",
                "本阶段开始后放置八段传送带，规划自己的工厂动线。",
                Goal::Build("belt", 8)
            ),
            step!(
                "第一批订单",
                "交付工坊的第一批精密部件，重新赢得客户信任。",
                Goal::Deliver(&[("gear", 10), ("circuit", 6), ("plate", 4)])
            ),
        ],
    },
    Expedition {
        id: "rescue",
        name: "无人应答的频率",
        desc: "一次地面求救，将你引向空间站之外。",
        rank: 1,
        steps: &[
            step!(
                "准备起航",
                "发送燃料与急救用氧，为搜救船准备补给。",
                Goal::Deliver(&[("fuel", 3), ("oxygen", 24)])
            ),
            step!(
                "接近中继站",
                "本阶段开始后停泊一次空间站，取得最后的求救坐标。",
                Goal::Event("docked", 1)
            ),
            step!(
                "清理威胁",
                "本阶段开始后击败一艘敌对海盗；和平访客不计入。",
                Goal::Event("pirateDefeated", 1)
            ),
            step!(
                "带他们回家",
                "向救援队发送医疗包和压缩氧气，让幸存者安全返航。",
                Goal::Deliver(&[("medkit", 3), ("oxygen_cell", 3)])
            ),
        ],
    },
    Expedition {
        id: "atlas",
        name: "群星的颜色",
        desc: "用实地样本，拼出一幅有生命的星图。",
        rank: 1,
        steps: &[
            step!(
                "陌生的风",
                "提交两种不同生态的调查报告；此前调查同样有效。",
                Goal::Survey(2)
            ),
            step!(
                "扩大视野",
                "研究扫描增幅 II，为深处的信号做好准备。",
                Goal::Research("scan2")
            ),
            step!(
                "冷热之间",
                "交付来自极寒与灼热地貌的矿物。星系地图可查行星生态。",
                Goal::Deliver(&[("cryocrystal", 8), ("basalt_shard", 12)])
            ),
            step!(
                "四色星图",
                "提交四种生态报告。不同世界的声音终于汇入同一张星图。",
                Goal::Survey(4)
            ),
        ],
    },
    Expedition {
        id: "deep",
        name: "遗迹的回声",
        desc: "解开守卫与晶体之间的联系，重建古老的信号装置。",
        rank: 2,
        steps: &[
            step!(
                "进入禁区",
                "本阶段开始后击败两名遗迹守卫；基地炮塔协助击杀也计入。",
                Goal::Event("sentinelDefeated", 2)
            ),
            step!(
                "重构信号",
                "用先进电路和超导体修复残存的数据链路。",
                Goal::Deliver(&[("advanced_circuit", 4), ("superconductor", 3)])
            ),
            step!(
                "异星样本",
                "交付菌类与蜂巢生态中的生物材料。",
                Goal::Deliver(&[("spores", 12), ("enzyme", 6)])
            ),
            step!(
                "回声阵列",
                "本阶段开始后部署三座信标，把遗迹信号接回文明。",
                Goal::Build("beacon", 3)
            ),
        ],
    },
    Expedition {
        id: "haven",
        name: "第二个家园",
        desc: "一条从深空航行到殖民地自给自足的完整远征。",
        rank: 3,
        steps: &[
            step!(
                "越过边界",
                "本阶段开始后完成一次星系跃迁。",
                Goal::Event("warpedOut", 1)
            ),
            step!(
                "定居物资",
                "向殖民计划交付批量生命保障物资。",
                Goal::Deliver(&[("oxygen_cell", 8), ("biofiber", 12), ("medkit", 4)])
            ),
            step!(
                "建立防线",
                "本阶段开始后放置两座防御炮塔，为它们连接电网。",
                Goal::Build("turret", 2)
            ),
            step!(
                "有人生活的星球",
                "本阶段开始后让殖民核心在供电和补给齐备时完成一次产出。",
                Goal::Event("colonyOnline", 1)
            ),
        ],
    },
];

pub const RANKS: &[(&str, u32)] = &[
    ("见习拓荒者", 0),
    ("边疆伙伴", 40),
    ("资深开拓者", 120),
    ("星际领航员", 260),
    ("星穹先驱", 500),
];
const EVENTS: &[&str] = &[
    "launched",
    "docked",
    "traded",
    "newPlanet",
    "warpedOut",
    "pirateDefeated",
    "sentinelDefeated",
    "colonyOnline",
];

#[derive(Default, Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct RouteProgress {
    pub stage: usize,
    pub baseline: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Sample {
    pub seed: u32,
    pub x: f32,
    pub z: f32,
}

#[derive(Default, Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Frontier {
    pub reputation: u32,
    pub completed_orders: u32,
    pub cycles: [u32; 3],
    pub active: [Option<String>; 3],
    pub routes: BTreeMap<String, RouteProgress>,
    pub tracked: Option<String>,
    pub events: BTreeMap<String, u32>,
    pub samples: BTreeMap<String, Vec<Sample>>,
    pub surveyed: BTreeSet<String>,
    pub milestones: BTreeSet<String>,
    pub village_deliveries: u32,
}

pub fn item_name(id: &str) -> &str {
    data::item_by_key(id).map_or(id, |i| i.name)
}

pub fn cargo_label(cargo: &[(&str, i32)]) -> String {
    cargo
        .iter()
        .map(|(id, n)| format!("{} ×{n}", item_name(id)))
        .collect::<Vec<_>>()
        .join(" · ")
}

/// Pay all cargo and grant all reward items, or leave the entire transaction
/// unchanged. Space freed by the delivery can hold its reward.
fn transact(
    p: &mut Player,
    costs: &[(&str, i32)],
    credits: i32,
    research: i32,
) -> Result<(), &'static str> {
    if p.creative() || p.dead {
        return Err("请在生存模式且存活时结算");
    }
    let mut inv: Inventory = p.inv.clone();
    if !inv.pay_items(costs) {
        return Err("交付物资不足；只检查角色背包");
    }
    if inv.add_item("data", research) != research {
        return Err("请为研究数据留出背包空间");
    }
    p.inv = inv;
    p.credits = p.credits.saturating_add(credits);
    Ok(())
}

impl Frontier {
    pub fn rank(&self) -> usize {
        RANKS
            .iter()
            .rposition(|(_, need)| self.reputation >= *need)
            .unwrap_or(0)
    }

    pub fn record_event(&mut self, id: &str) {
        if EVENTS.contains(&id) {
            let n = self.events.entry(id.into()).or_default();
            *n = n.saturating_add(1);
        }
    }

    pub fn offers(&self, seed: u32, techs: &[String]) -> [Option<&'static Contract>; 3] {
        let pool: Vec<_> = CONTRACTS
            .iter()
            .filter(|c| c.rank <= self.rank() && c.unlocked(techs))
            .collect();
        let mut chosen = BTreeSet::new();
        for id in self.active.iter().flatten() {
            chosen.insert(id.as_str());
        }
        std::array::from_fn(|slot| {
            if let Some(id) = &self.active[slot] {
                return CONTRACTS.iter().find(|c| c.id == id);
            }
            let mut rng = crate::rng::Rng::new(
                seed ^ self.cycles[slot].wrapping_mul(0x9E3779B9)
                    ^ (slot as u32 + 1).wrapping_mul(0x85EBCA6B),
            );
            let start = rng.range(pool.len().max(1));
            for i in 0..pool.len() {
                let c = pool[(start + i) % pool.len()];
                if chosen.insert(c.id) {
                    return Some(c);
                }
            }
            None
        })
    }

    pub fn accept(&mut self, slot: usize, seed: u32, techs: &[String]) -> Result<(), &'static str> {
        if slot >= 3 || self.active[slot].is_some() {
            return Err("委托槽位不可用");
        }
        let id = self.offers(seed, techs)[slot].ok_or("暂无可接委托")?.id;
        self.active[slot] = Some(id.into());
        Ok(())
    }

    pub fn finish_order(&mut self, slot: usize, p: &mut Player) -> Result<(), &'static str> {
        let id = self
            .active
            .get(slot)
            .and_then(|s| s.as_deref())
            .ok_or("请先接受委托")?;
        let c = CONTRACTS.iter().find(|c| c.id == id).ok_or("委托已失效")?;
        transact(p, c.cargo, c.credits(), c.rank as i32 + 1)?;
        self.reputation = self.reputation.saturating_add(8 + c.rank as u32 * 4);
        self.completed_orders = self.completed_orders.saturating_add(1);
        self.active[slot] = None;
        self.cycles[slot] = self.cycles[slot].wrapping_add(1);
        Ok(())
    }

    pub fn cancel(&mut self, slot: usize) {
        if slot < 3 && self.active[slot].take().is_some() {
            self.cycles[slot] = self.cycles[slot].wrapping_add(1);
        }
    }

    pub fn scan(&mut self, biome: &str, sample: Sample) -> bool {
        if !data::BIOMES.iter().any(|b| b.key == biome)
            || !sample.x.is_finite()
            || !sample.z.is_finite()
        {
            return false;
        }
        let points = self.samples.entry(biome.into()).or_default();
        if points.len() >= 3
            || points
                .iter()
                .any(|p| p.seed == sample.seed && (p.x - sample.x).hypot(p.z - sample.z) < 64.0)
        {
            return false;
        }
        points.push(sample);
        true
    }

    pub fn submit_survey(&mut self, biome: &str, p: &mut Player) -> Result<(), &'static str> {
        if self.surveyed.contains(biome) {
            return Err("该生态报告已提交");
        }
        if self.samples.get(biome).map_or(0, Vec::len) < 3 {
            return Err("需要三处相距至少 64m 的扫描记录");
        }
        let (item, n, _) = survey_spec(biome).ok_or("未知生态")?;
        let hazardous = data::biome_by_key(biome).haz.is_some();
        transact(
            p,
            &[(item, n)],
            if hazardous { 450 } else { 250 },
            if hazardous { 6 } else { 4 },
        )?;
        self.surveyed.insert(biome.into());
        self.reputation = self.reputation.saturating_add(15);
        Ok(())
    }

    fn counter(&self, goal: Goal, placed: &HashMap<String, i32>) -> u32 {
        match goal {
            Goal::Event(id, _) => self.events.get(id).copied().unwrap_or(0),
            Goal::Build(id, _) => placed.get(id).copied().unwrap_or(0).max(0) as u32,
            _ => 0,
        }
    }

    pub fn start_route(
        &mut self,
        id: &str,
        placed: &HashMap<String, i32>,
    ) -> Result<(), &'static str> {
        let route = EXPEDITIONS.iter().find(|r| r.id == id).ok_or("未知远征")?;
        if self.rank() < route.rank {
            return Err("公会等级不足");
        }
        let baseline = self.counter(route.steps[0].goal, placed);
        self.routes
            .entry(id.into())
            .or_insert(RouteProgress { stage: 0, baseline });
        self.tracked = Some(id.into());
        Ok(())
    }

    pub fn route_progress(
        &self,
        route: &Expedition,
        p: &Player,
        placed: &HashMap<String, i32>,
        techs: &[String],
    ) -> (u32, u32) {
        let Some(progress) = self.routes.get(route.id) else {
            return (0, 1);
        };
        let Some(step) = route.steps.get(progress.stage) else {
            return (1, 1);
        };
        match step.goal {
            Goal::Deliver(costs) => (
                costs
                    .iter()
                    .map(|(id, n)| p.inv.count_item(id).min(*n).max(0) as u32)
                    .sum(),
                costs.iter().map(|(_, n)| *n as u32).sum(),
            ),
            Goal::Event(_, n) | Goal::Build(_, n) => (
                self.counter(step.goal, placed)
                    .saturating_sub(progress.baseline)
                    .min(n),
                n,
            ),
            Goal::Survey(n) => ((self.surveyed.len() as u32).min(n), n),
            Goal::Research(id) => (u32::from(data::tech_unlocked(techs, id)), 1),
        }
    }

    pub fn advance_route(
        &mut self,
        id: &str,
        p: &mut Player,
        placed: &HashMap<String, i32>,
        techs: &[String],
    ) -> Result<(), &'static str> {
        let route = EXPEDITIONS.iter().find(|r| r.id == id).ok_or("未知远征")?;
        let stage = self.routes.get(id).ok_or("请先开始远征")?.stage;
        let step = route.steps.get(stage).ok_or("该远征已完成")?;
        let (have, need) = self.route_progress(route, p, placed, techs);
        if have < need {
            return Err("阶段目标尚未达成");
        }
        let costs = match step.goal {
            Goal::Deliver(c) => c,
            _ => &[],
        };
        let last = stage + 1 == route.steps.len();
        transact(
            p,
            costs,
            if last {
                1200 + route.rank as i32 * 400
            } else {
                200
            },
            if last { 8 } else { 2 },
        )?;
        self.reputation = self.reputation.saturating_add(if last { 30 } else { 5 });
        let baseline = route
            .steps
            .get(stage + 1)
            .map_or(0, |s| self.counter(s.goal, placed));
        self.routes.insert(
            id.into(),
            RouteProgress {
                stage: stage + 1,
                baseline,
            },
        );
        Ok(())
    }

    pub fn completed_routes(&self) -> u32 {
        EXPEDITIONS
            .iter()
            .filter(|r| {
                self.routes
                    .get(r.id)
                    .is_some_and(|p| p.stage >= r.steps.len())
            })
            .count() as u32
    }

    pub fn milestone_progress(&self, m: &Milestone) -> u32 {
        match m.metric {
            Metric::Orders => self.completed_orders,
            Metric::Surveys => self.surveyed.len() as u32,
            Metric::Routes => self.completed_routes(),
            Metric::Event(id) => self.events.get(id).copied().unwrap_or(0),
        }
        .min(m.need)
    }

    pub fn claim_milestone(&mut self, id: &str, p: &mut Player) -> Result<(), &'static str> {
        let m = MILESTONES.iter().find(|m| m.id == id).ok_or("未知里程碑")?;
        if self.milestones.contains(id) {
            return Err("奖励已经领取");
        }
        if self.milestone_progress(m) < m.need {
            return Err("里程碑尚未达成");
        }
        transact(p, &[], m.credits, m.data)?;
        self.milestones.insert(id.into());
        Ok(())
    }

    pub fn sanitize(&mut self) {
        self.reputation = self.reputation.min(1_000_000);
        self.completed_orders = self.completed_orders.min(1_000_000);
        let mut ids = BTreeSet::new();
        for slot in &mut self.active {
            if let Some(id) = slot.as_ref()
                && (!CONTRACTS.iter().any(|c| c.id == id) || !ids.insert(id.clone()))
            {
                *slot = None;
            }
        }
        self.events.retain(|id, n| {
            *n = (*n).min(1_000_000);
            EVENTS.contains(&id.as_str())
        });
        self.routes.retain(|id, progress| {
            if let Some(r) = EXPEDITIONS.iter().find(|r| r.id == id) {
                progress.stage = progress.stage.min(r.steps.len());
                true
            } else {
                false
            }
        });
        if self
            .tracked
            .as_ref()
            .is_some_and(|id| !self.routes.contains_key(id))
        {
            self.tracked = None;
        }
        let old = std::mem::take(&mut self.samples);
        for (biome, points) in old {
            for point in points.into_iter().take(64) {
                self.scan(&biome, point);
            }
        }
        self.surveyed.retain(|id| survey_spec(id).is_some());
        self.milestones
            .retain(|id| MILESTONES.iter().any(|m| m.id == id));
    }
}

/// Materials already obtainable from each biome's terrain/vegetation drops.
pub fn survey_spec(biome: &str) -> Option<(&'static str, i32, &'static str)> {
    Some(match biome {
        "lush" => ("carbon", 12, "植被中的碳循环维系着第一批地表生命。"),
        "desert" => ("sand", 16, "风把石英打磨成沙，也留下了古海洋的轮廓。"),
        "frozen" => ("cryocrystal", 6, "冰层中的晶体保存着漫长极夜的温度。"),
        "volcanic" => (
            "basalt_shard",
            8,
            "新生的岩石记录着这颗星球尚未冷却的心脏。",
        ),
        "alien" => (
            "spores",
            8,
            "荧光孢子借风传播，让整片大地像神经网络一样呼吸。",
        ),
        "ocean" => ("sand", 16, "岛屿是海底山脉露出水面的顶端。"),
        "crystal" => ("cryocrystal", 8, "晶簇把极光折射进永冻层深处。"),
        "fungal" => ("spores", 10, "巨菌以地下菌丝共享水分，形成一座无声的森林。"),
        "ashen" => ("basalt_shard", 10, "灰烬之下，顽强的微生物开始重建土壤。"),
        "amber" => ("resin", 8, "金色树脂封存着风暴前的古老生态。"),
        "ferrous" => (
            "iron_ore",
            16,
            "富铁尘埃沿磁力线迁移，地表本身就是一张罗盘。",
        ),
        "murk" => ("enzyme", 6, "沼泽酶能分解复杂物质，是异星医药的宝贵线索。"),
        "salt" => ("salt_crystal", 10, "蒸发海留下盐层，也留下曾经宜居的证据。"),
        "obsidian" => (
            "basalt_shard",
            12,
            "骤冷的熔岩形成玻璃质岩壁，裂隙仍散发热量。",
        ),
        "redmoss" => ("carbon", 16, "红藓以低矮姿态抵抗稀薄大气中的寒风。"),
        "hive" => ("chitin", 8, "蜂窝结构让微小的生命共同建起宏大的穹丘。"),
        _ => return None,
    })
}

pub enum Metric {
    Orders,
    Surveys,
    Routes,
    Event(&'static str),
}
pub struct Milestone {
    pub id: &'static str,
    pub name: &'static str,
    pub desc: &'static str,
    pub metric: Metric,
    pub need: u32,
    pub credits: i32,
    pub data: i32,
}

macro_rules! milestone {
    ($id:literal, $name:literal, $desc:literal, $metric:expr, $need:literal, $credits:literal, $data:literal) => {
        Milestone {
            id: $id,
            name: $name,
            desc: $desc,
            metric: $metric,
            need: $need,
            credits: $credits,
            data: $data,
        }
    };
}

pub const MILESTONES: &[Milestone] = &[
    milestone!(
        "first_order",
        "说到做到",
        "完成 1 份公会委托",
        Metric::Orders,
        1,
        150,
        2
    ),
    milestone!(
        "supplier",
        "可靠供应商",
        "完成 10 份公会委托",
        Metric::Orders,
        10,
        800,
        6
    ),
    milestone!(
        "logistician",
        "边疆生命线",
        "完成 30 份公会委托",
        Metric::Orders,
        30,
        2400,
        12
    ),
    milestone!(
        "naturalist",
        "第一份田野笔记",
        "提交 1 种生态调查",
        Metric::Surveys,
        1,
        200,
        2
    ),
    milestone!(
        "atlas",
        "多彩宇宙",
        "提交 8 种生态调查",
        Metric::Surveys,
        8,
        1800,
        12
    ),
    milestone!(
        "encyclopedia",
        "活的百科全书",
        "提交全部 16 种生态调查",
        Metric::Surveys,
        16,
        5000,
        24
    ),
    milestone!(
        "story",
        "一个完整的故事",
        "完成 1 条远征",
        Metric::Routes,
        1,
        600,
        4
    ),
    milestone!(
        "pioneer",
        "边疆编年史",
        "完成全部 6 条远征",
        Metric::Routes,
        6,
        4000,
        20
    ),
    milestone!(
        "guardian",
        "航路守望者",
        "击败 5 艘敌对海盗",
        Metric::Event("pirateDefeated"),
        5,
        1400,
        8
    ),
    milestone!(
        "ruins",
        "遗迹破译者",
        "击败 6 名遗迹守卫",
        Metric::Event("sentinelDefeated"),
        6,
        900,
        6
    ),
    milestone!(
        "voyager",
        "跨越群星",
        "完成 3 次星系跃迁",
        Metric::Event("warpedOut"),
        3,
        1600,
        10
    ),
    milestone!(
        "settler",
        "持续的灯火",
        "殖民核心完成 5 次产出",
        Metric::Event("colonyOnline"),
        5,
        1800,
        10
    ),
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::Slot;

    fn player() -> Player {
        Player::new(data::Difficulty::Normal)
    }
    fn all_techs() -> Vec<String> {
        data::TECHS.iter().map(|t| t.id.into()).collect()
    }
    fn roundtrip(f: &Frontier) -> Frontier {
        serde_json::from_slice(&serde_json::to_vec(f).unwrap()).unwrap()
    }

    #[test]
    fn all_content_references_are_valid_and_have_real_sources() {
        let mut ids = BTreeSet::new();
        for c in CONTRACTS {
            assert!(ids.insert(c.id));
            assert!(c.rank < RANKS.len());
            assert!(data::TECHS.iter().any(|t| t.id == c.tech));
            assert!(c.unlocked(&all_techs()));
            assert!(c.credits() > 0);
            for (id, n) in c.cargo {
                assert!(data::item_by_key(id).is_some() && *n > 0, "{}: {id}", c.id);
            }
        }
        ids.clear();
        for r in EXPEDITIONS {
            assert!(ids.insert(r.id));
            assert!(r.rank < RANKS.len());
            assert_eq!(r.steps.len(), 4);
            for s in r.steps {
                match s.goal {
                    Goal::Deliver(costs) => {
                        for (id, n) in costs {
                            assert!(data::item_by_key(id).is_some() && *n > 0);
                        }
                    }
                    Goal::Event(id, n) => assert!(EVENTS.contains(&id) && n > 0),
                    Goal::Build(id, n) => {
                        assert!(data::BLOCKS.iter().any(|b| b.key == id) && n > 0)
                    }
                    Goal::Research(id) => assert!(data::TECHS.iter().any(|t| t.id == id)),
                    Goal::Survey(n) => assert!(n > 0 && n <= data::BIOMES.len() as u32),
                }
            }
        }
        for biome in data::BIOMES {
            let (item, n, _) = survey_spec(biome.key).expect(biome.key);
            assert!(data::item_by_key(item).is_some() && n > 0);
            assert!(
                data::BLOCKS
                    .iter()
                    .any(|b| b.drops.iter().any(|d| d.item == item)),
                "survey sample {item} needs a mineable source"
            );
        }
        ids.clear();
        for m in MILESTONES {
            assert!(ids.insert(m.id) && m.need > 0 && m.credits > 0 && m.data > 0);
        }
        assert_eq!(CONTRACTS.len(), 30);
        assert_eq!(EXPEDITIONS.len(), 6);
        assert_eq!(MILESTONES.len(), 12);
    }

    #[test]
    fn offers_are_deterministic_distinct_tech_gated_and_all_reachable() {
        let f = Frontier::default();
        for seed in 0..100 {
            let offers = f.offers(seed, &[]);
            let ids: Vec<_> = offers.iter().map(|c| c.unwrap().id).collect();
            assert_eq!(ids.iter().collect::<BTreeSet<_>>().len(), 3);
            assert_eq!(
                ids,
                f.offers(seed, &[])
                    .iter()
                    .map(|c| c.unwrap().id)
                    .collect::<Vec<_>>()
            );
            assert!(offers.iter().all(|c| c.unwrap().rank == 0));
        }
        let f = Frontier {
            reputation: 500,
            ..Default::default()
        };
        let mut seen = BTreeSet::new();
        for seed in 0..300 {
            for c in f.offers(seed, &all_techs()).iter().flatten() {
                seen.insert(c.id);
            }
        }
        assert_eq!(seen.len(), CONTRACTS.len());
        let c = CONTRACTS.iter().find(|c| c.id == "antimatter").unwrap();
        assert!(!c.unlocked(&["nuclear".into()])); // Ship alloy also needs ship systems.
    }

    #[test]
    fn accepted_orders_survive_rank_galaxy_and_save_changes() {
        let mut f = Frontier::default();
        f.accept(1, 42, &[]).unwrap();
        let id = f.active[1].clone().unwrap();
        f.reputation = 500;
        let restored = roundtrip(&f);
        let offers = restored.offers(9876, &all_techs());
        assert_eq!(offers[1].unwrap().id, id);
        assert_ne!(offers[0].unwrap().id, id);
        assert_ne!(offers[2].unwrap().id, id);
        assert!(f.accept(1, 42, &[]).is_err());
        assert!(f.accept(3, 42, &[]).is_err());
    }

    #[test]
    fn failed_delivery_is_atomic_for_missing_cargo_and_full_rewards() {
        let mut f = Frontier::default();
        f.active[0] = Some("camp".into());
        let mut p = player();
        let inv = p.inv.slots.clone();
        let credits = p.credits;
        assert!(f.finish_order(0, &mut p).is_err());
        assert_eq!(p.inv.slots, inv);
        p.inv.slots = vec![
            Some(Slot {
                item: "stone".into(),
                n: 250
            });
            36
        ];
        p.inv.slots[0] = Some(Slot {
            item: "carbon".into(),
            n: 250,
        });
        let inv = p.inv.slots.clone();
        assert!(f.finish_order(0, &mut p).is_err());
        assert_eq!(p.inv.slots, inv);
        assert_eq!(p.credits, credits);
        assert_eq!(f.completed_orders, 0);
        assert_eq!(f.reputation, 0);
        assert_eq!(f.cycles, [0; 3]);
        assert!(f.active[0].is_some());
    }

    #[test]
    fn freed_delivery_slot_can_hold_reward_and_cannot_pay_twice() {
        let mut f = Frontier::default();
        f.active[0] = Some("fuel".into());
        let mut p = player();
        p.inv.slots = vec![Some(Slot {
            item: "fuel".into(),
            n: 2,
        })];
        f.finish_order(0, &mut p).unwrap();
        assert_eq!(p.inv.count_item("fuel"), 0);
        assert_eq!(p.inv.count_item("data"), 1);
        assert_eq!(f.completed_orders, 1);
        assert_eq!(f.cycles[0], 1);
        let credits = p.credits;
        assert!(f.finish_order(0, &mut p).is_err());
        assert_eq!(p.credits, credits);
        f.cancel(0);
        f.cancel(99);
        assert_eq!(f.cycles[0], 1);
    }

    #[test]
    fn creative_and_dead_players_cannot_turn_free_items_into_progress() {
        for creative in [true, false] {
            let mut p = if creative {
                Player::new(data::Difficulty::Creative)
            } else {
                player()
            };
            p.dead = !creative;
            p.inv.add_item("fuel", 2);
            let mut f = Frontier::default();
            f.active[0] = Some("fuel".into());
            assert!(f.finish_order(0, &mut p).is_err());
            assert_eq!(f.completed_orders, 0);
            assert_eq!(p.inv.count_item("fuel"), 2);
        }
    }

    #[test]
    fn survey_requires_separated_horizontal_samples_and_is_bounded() {
        let mut f = Frontier::default();
        assert!(f.scan(
            "lush",
            Sample {
                seed: 1,
                x: 0.0,
                z: 0.0
            }
        ));
        assert!(!f.scan(
            "lush",
            Sample {
                seed: 1,
                x: 63.9,
                z: 0.0
            }
        ));
        assert!(f.scan(
            "lush",
            Sample {
                seed: 1,
                x: 64.0,
                z: 0.0
            }
        ));
        assert!(f.scan(
            "lush",
            Sample {
                seed: 2,
                x: 0.0,
                z: 0.0
            }
        ));
        assert!(!f.scan(
            "lush",
            Sample {
                seed: 3,
                x: 0.0,
                z: 0.0
            }
        ));
        assert!(!f.scan(
            "unknown",
            Sample {
                seed: 1,
                x: 0.0,
                z: 0.0
            }
        ));
        assert!(!f.scan(
            "desert",
            Sample {
                seed: 1,
                x: f32::NAN,
                z: 0.0
            }
        ));
        let mut p = player();
        p.inv = Inventory::default();
        assert!(f.submit_survey("lush", &mut p).is_err());
        p.inv.add_item("carbon", 12);
        f.submit_survey("lush", &mut p).unwrap();
        assert_eq!(p.inv.count_item("carbon"), 0);
        assert_eq!(p.inv.count_item("data"), 4);
        let mut restored = roundtrip(&f);
        assert!(restored.submit_survey("lush", &mut p).is_err());
        assert_eq!(restored.surveyed.len(), 1);
        assert_eq!(restored.reputation, 15);
    }

    #[test]
    fn expedition_events_require_fresh_activity_and_restart_does_not_reset() {
        let mut f = Frontier {
            reputation: 500,
            ..Default::default()
        };
        f.record_event("warpedOut");
        let placed = HashMap::new();
        f.start_route("haven", &placed).unwrap();
        let route = EXPEDITIONS.iter().find(|r| r.id == "haven").unwrap();
        let mut p = player();
        assert_eq!(f.route_progress(route, &p, &placed, &[]), (0, 1));
        assert!(f.advance_route("haven", &mut p, &placed, &[]).is_err());
        f.record_event("warpedOut");
        let mut f = roundtrip(&f);
        f.start_route("haven", &placed).unwrap();
        assert_eq!(f.route_progress(route, &p, &placed, &[]), (1, 1));
        f.advance_route("haven", &mut p, &placed, &[]).unwrap();
        f.start_route("haven", &placed).unwrap();
        assert_eq!(f.routes["haven"].stage, 1);
        assert!(f.advance_route("haven", &mut p, &placed, &[]).is_err());
        assert_eq!(f.events["warpedOut"], 2);
    }

    #[test]
    fn every_expedition_can_finish_once_with_persisted_stage_boundaries() {
        let mut f = Frontier {
            reputation: 500,
            ..Default::default()
        };
        let mut p = player();
        let mut placed = HashMap::new();
        let techs = all_techs();
        for route in EXPEDITIONS {
            f.start_route(route.id, &placed).unwrap();
            for step in route.steps {
                p.inv = Inventory::default();
                match step.goal {
                    Goal::Deliver(costs) => {
                        for (item, n) in costs {
                            p.inv.add_item(item, *n);
                        }
                    }
                    Goal::Event(id, n) => {
                        for _ in 0..n {
                            f.record_event(id);
                        }
                    }
                    Goal::Build(id, n) => *placed.entry(id.into()).or_default() += n as i32,
                    Goal::Survey(n) => {
                        f.surveyed
                            .extend(data::BIOMES.iter().take(n as usize).map(|b| b.key.into()));
                    }
                    Goal::Research(_) => {}
                }
                f.advance_route(route.id, &mut p, &placed, &techs).unwrap();
                f = roundtrip(&f);
            }
            let credits = p.credits;
            f.start_route(route.id, &placed).unwrap();
            assert!(f.advance_route(route.id, &mut p, &placed, &techs).is_err());
            assert_eq!(p.credits, credits);
        }
        assert_eq!(f.completed_routes(), 6);
    }

    #[test]
    fn build_stage_does_not_count_previous_construction() {
        let mut f = Frontier::default();
        let mut p = player();
        p.inv.add_item("oxygen", 12);
        p.inv.add_item("carbon", 24);
        let mut placed = HashMap::from([("beacon".into(), 10)]);
        f.start_route("shelter", &placed).unwrap();
        f.advance_route("shelter", &mut p, &placed, &[]).unwrap();
        assert!(f.advance_route("shelter", &mut p, &placed, &[]).is_err());
        placed.insert("beacon".into(), 11);
        f.advance_route("shelter", &mut p, &placed, &[]).unwrap();
        assert_eq!(f.routes["shelter"].stage, 2);
    }

    #[test]
    fn milestone_reward_waits_for_inventory_and_survives_reload() {
        let mut f = Frontier {
            completed_orders: 1,
            ..Default::default()
        };
        let mut p = player();
        p.inv.slots = vec![
            Some(Slot {
                item: "stone".into(),
                n: 250
            });
            36
        ];
        assert!(f.claim_milestone("first_order", &mut p).is_err());
        assert!(f.milestones.is_empty());
        p.inv.slots[0] = None;
        f.claim_milestone("first_order", &mut p).unwrap();
        let credits = p.credits;
        assert!(
            roundtrip(&f)
                .claim_milestone("first_order", &mut p)
                .is_err()
        );
        assert_eq!(p.credits, credits);
    }

    #[test]
    fn sanitization_removes_invalid_records_and_duplicate_samples() {
        let mut f: Frontier = serde_json::from_value(serde_json::json!({
            "active": ["air", "air", "not_real"], "tracked": "gone",
            "routes": { "gone": {"stage": 0} },
            "samples": { "lush": [ {"seed": 1, "x": 0.0, "z": 0.0}, {"seed": 1, "x": 0.0, "z": 0.0} ] },
            "events": { "fake": 99, "docked": 2 }, "surveyed": ["fake"], "milestones": ["fake"]
        })).unwrap();
        f.sanitize();
        assert_eq!(f.active, [Some("air".into()), None, None]);
        assert!(f.tracked.is_none() && f.routes.is_empty());
        assert_eq!(f.samples["lush"].len(), 1);
        assert_eq!(f.events.len(), 1);
        assert!(f.surveyed.is_empty() && f.milestones.is_empty());
    }
}
