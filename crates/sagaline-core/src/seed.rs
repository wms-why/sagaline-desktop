//! Seed data — bootstrap a workspace with a rich, relationship-driven demo
//! story so the UI shows non-empty content on first launch.

use rusqlite::Connection;

use crate::db;
use crate::model::*;

/// If the database has no stories, create one with characters, environments,
/// chapters, scenes, and shots so the workspace page renders relationships.
/// Returns the new story's id; returns `None` if a story already exists.
pub fn ensure_demo_story(conn: &Connection) -> Result<Option<StoryId>, db::Error> {
    let existing = db::list_stories(conn)?;
    if !existing.is_empty() {
        return Ok(None);
    }
    let story = db::create_story(conn, "末日之城")?;
    seed_story(conn, &story)?;
    Ok(Some(story.id))
}

pub fn seed_story(conn: &Connection, story: &Story) -> Result<(), db::Error> {
    // ---------------- Story Bible ----------------
    let mut bible = db::get_bible(conn, &story.id)?;
    bible.world = "2057 年。全球能源系统全面崩溃，欧洲进入冰封时代，\
        中国南方部分城市依靠地下能源维持运转。AI 被立法禁止自主决策，\
        所有智能体必须接受人类监督。"
        .into();
    bible.rules = "AI 不得自主决策；能源核心一旦过载将导致半径 5 km 内所有电子设备永久失效；\
        离开深圳地下城超过 72 小时将失去能源签证。"
        .into();
    bible.timeline = "故事起点：2057-03-01".into();
    bible.lore = "深圳地下城是亚洲最大的能源避难所，由林氏财团与南方联邦共建。\
        整个故事围绕 M-01 能源核心展开。"
        .into();
    db::save_bible(conn, &bible)?;

    // ---------------- Characters ----------------
    let lin_mo = Character {
        id: CharacterId::new(),
        story_id: story.id.clone(),
        name: "林默".into(),
        profile: "28 岁，前 AI 工程师，深圳地下城首席能源架构师。".into(),
        personality: "冷静、理性、固执；不善言辞但行动果断。".into(),
        background: "曾参与秘密 AI 项目“守夜人”，目睹 AI 失控导致导师失踪。".into(),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    db::add_character(conn, &lin_mo)?;

    let age_28 = CharacterAge {
        id: CharacterAgeId::new(),
        character_id: lin_mo.id.clone(),
        age: 28,
        description: "成年主年龄段，承担故事核心剧情。".into(),
        default_reference: None,
        created_at: chrono::Utc::now(),
    };
    db::add_character_age(conn, &age_28)?;

    let app_casual = CharacterAppearance {
        id: CharacterAppearanceId::new(),
        age_id: age_28.id.clone(),
        name: "日常".into(),
        description: "深圳地下城日常便装".into(),
        clothing: "深灰色连帽外套，黑色长裤".into(),
        hairstyle: "短发".into(),
        accessories: "腕带终端".into(),
        emotion: "平静".into(),
        body_state: "健康".into(),
        created_at: chrono::Utc::now(),
    };
    db::add_character_appearance(conn, &app_casual)?;

    let app_combat = CharacterAppearance {
        id: CharacterAppearanceId::new(),
        age_id: age_28.id.clone(),
        name: "战斗服".into(),
        description: "灾难应对战术装备".into(),
        clothing: "黑色战术外套，带能源指示灯".into(),
        hairstyle: "短发".into(),
        accessories: "手枪、能源模块".into(),
        emotion: "警惕".into(),
        body_state: "轻微受伤".into(),
        created_at: chrono::Utc::now(),
    };
    db::add_character_appearance(conn, &app_combat)?;

    let su_yan = Character {
        id: CharacterId::new(),
        story_id: story.id.clone(),
        name: "苏妍".into(),
        profile: "26 岁，深圳地下城医疗官，林默的恋人。".into(),
        personality: "温柔但坚定，共情能力强。".into(),
        background: "战地医生世家出身，第一批地下城居民。".into(),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    db::add_character(conn, &su_yan)?;

    let su_age = CharacterAge {
        id: CharacterAgeId::new(),
        character_id: su_yan.id.clone(),
        age: 26,
        description: "成年主年龄段。".into(),
        default_reference: None,
        created_at: chrono::Utc::now(),
    };
    db::add_character_age(conn, &su_age)?;
    db::add_character_appearance(conn, &CharacterAppearance {
        id: CharacterAppearanceId::new(),
        age_id: su_age.id.clone(),
        name: "医疗服".into(),
        description: "战地医疗".into(),
        clothing: "白色医疗大褂".into(),
        hairstyle: "长发束起".into(),
        accessories: "医疗手环".into(),
        emotion: "温柔".into(),
        body_state: "健康".into(),
        created_at: chrono::Utc::now(),
    })?;

    let dr_chen = Character {
        id: CharacterId::new(),
        story_id: story.id.clone(),
        name: "陈博士".into(),
        profile: "62 岁，深圳地下城 AI 伦理委员会主席，林默的导师。".into(),
        personality: "睿智、谨慎，对 AI 抱有复杂情感。".into(),
        background: "“守夜人”项目缔造者，2057 年失踪。".into(),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    db::add_character(conn, &dr_chen)?;
    let chen_age = CharacterAge {
        id: CharacterAgeId::new(),
        character_id: dr_chen.id.clone(),
        age: 62,
        description: "老年，导师角色".into(),
        default_reference: None,
        created_at: chrono::Utc::now(),
    };
    db::add_character_age(conn, &chen_age)?;
    db::add_character_appearance(conn, &CharacterAppearance {
        id: CharacterAppearanceId::new(),
        age_id: chen_age.id.clone(),
        name: "学者".into(),
        description: "实验室装".into(),
        clothing: "深色西装外套".into(),
        hairstyle: "灰白短发".into(),
        accessories: "老式眼镜".into(),
        emotion: "沉思".into(),
        body_state: "健康".into(),
        created_at: chrono::Utc::now(),
    })?;

    // ---------------- Relationships ----------------
    db::add_relationship(conn, &Relationship {
        id: RelationshipId::new(),
        story_id: story.id.clone(),
        from_character: lin_mo.id.clone(),
        to_character: su_yan.id.clone(),
        kind: "lover".into(),
        status: RelationshipStatus::Active,
        note: "恋人，长期驻扎深圳地下城。".into(),
    })?;
    db::add_relationship(conn, &Relationship {
        id: RelationshipId::new(),
        story_id: story.id.clone(),
        from_character: lin_mo.id.clone(),
        to_character: dr_chen.id.clone(),
        kind: "mentor".into(),
        status: RelationshipStatus::Broken,
        note: "导师关系因守夜人事件破裂，陈博士失踪后断裂。".into(),
    })?;

    // ---------------- Environments ----------------
    let lab = Environment {
        id: EnvironmentId::new(),
        story_id: story.id.clone(),
        name: "AI 实验室".into(),
        description: "深圳地下城核心 AI 研究设施。".into(),
        architecture: "极简金属风，嵌入式服务器机柜。".into(),
        lighting: "蓝色背光，警告时变红。".into(),
        weather: String::new(),
        time_of_day: "夜".into(),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    db::add_environment(conn, &lab)?;

    let garage = Environment {
        id: EnvironmentId::new(),
        story_id: story.id.clone(),
        name: "地下停车场".into(),
        description: "连接地下城与地表出口的主要通道。".into(),
        architecture: "混凝土粗犷结构，悬挂式应急灯。".into(),
        lighting: "昏黄荧光灯，闪烁不稳。".into(),
        weather: String::new(),
        time_of_day: "夜".into(),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    db::add_environment(conn, &garage)?;

    let street = Environment {
        id: EnvironmentId::new(),
        story_id: story.id.clone(),
        name: "深圳街道".into(),
        description: "地表已断电的城市干道。".into(),
        architecture: "现代高楼与废弃店铺并存。".into(),
        lighting: "月光与远处火光。".into(),
        weather: "细雨".into(),
        time_of_day: "夜".into(),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    db::add_environment(conn, &street)?;

    // ---------------- Props ----------------
    let core = Prop {
        id: PropId::new(),
        story_id: story.id.clone(),
        name: "M-01 能源核心".into(),
        description: "拳头大小的蓝色能源核心。".into(),
        appearance: "透明外壳，内部有蓝色能量流动".into(),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    db::add_prop(conn, &core)?;

    // ---------------- Chapter 1 ----------------
    let chapter = Chapter {
        id: ChapterId::new(),
        story_id: story.id.clone(),
        chapter_number: 1,
        title: "灾难开始".into(),
        summary: "林默发现能源系统即将崩溃，整个城市陷入停电。".into(),
        story: "晚上 10:30，林默在 AI 实验室例行巡检时发现 M-01 能源核心读数异常。\
            几分钟后，城市电力系统出现连锁故障。"
            .into(),
        timeline: "2057-03-01 夜".into(),
        status: ChapterStatus::Draft,
        generated_video: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    db::add_chapter(conn, &chapter)?;
    db::link_chapter_character(conn, &chapter.id, &lin_mo.id)?;
    db::link_chapter_character(conn, &chapter.id, &dr_chen.id)?;
    db::link_chapter_environment(conn, &chapter.id, &lab.id)?;
    db::link_chapter_environment(conn, &chapter.id, &garage.id)?;
    db::link_chapter_environment(conn, &chapter.id, &street.id)?;

    // Scenes
    let scene_lab = Scene {
        id: SceneId::new(),
        chapter_id: chapter.id.clone(),
        scene_number: 1,
        title: "实验室".into(),
        description: "林默发现能源系统出现异常。".into(),
        environment_id: Some(lab.id.clone()),
        emotion: "紧张".into(),
        duration_seconds: 45,
        created_at: chrono::Utc::now(),
    };
    db::add_scene(conn, &scene_lab)?;

    let scene_garage = Scene {
        id: SceneId::new(),
        chapter_id: chapter.id.clone(),
        scene_number: 2,
        title: "地下停车场".into(),
        description: "林默紧急下到车库取出备用手枪。".into(),
        environment_id: Some(garage.id.clone()),
        emotion: "警觉".into(),
        duration_seconds: 30,
        created_at: chrono::Utc::now(),
    };
    db::add_scene(conn, &scene_garage)?;

    let scene_street = Scene {
        id: SceneId::new(),
        chapter_id: chapter.id.clone(),
        scene_number: 3,
        title: "深圳街道".into(),
        description: "林默目睹城市开始陷入停电。".into(),
        environment_id: Some(street.id.clone()),
        emotion: "震撼".into(),
        duration_seconds: 40,
        created_at: chrono::Utc::now(),
    };
    db::add_scene(conn, &scene_street)?;

    // Shots — lab scene
    let shot1 = Shot {
        id: ShotId::new(),
        scene_id: scene_lab.id.clone(),
        shot_number: 1,
        description: "实验室全景，服务器机柜亮起红色警示灯。".into(),
        camera: "Wide".into(),
        camera_movement: "Static".into(),
        composition: "Center".into(),
        action: "服务器机柜的灯带由蓝转红。".into(),
        dialogue: String::new(),
        emotion: "紧张".into(),
        duration_seconds: 5,
        environment_id: Some(lab.id.clone()),
        first_frame: None,
        last_frame: None,
        video_model: None,
        generation_settings: String::new(),
        generated_video: None,
        created_at: chrono::Utc::now(),
    };
    db::add_shot(conn, &shot1)?;
    db::link_shot_character(conn, &ShotCharacter {
        shot_id: shot1.id.clone(),
        character_id: lin_mo.id.clone(),
        age_id: age_28.id.clone(),
        appearance_id: app_casual.id.clone(),
    })?;
    db::link_shot_prop(conn, &shot1.id, &core.id)?;

    let shot2 = Shot {
        id: ShotId::new(),
        scene_id: scene_lab.id.clone(),
        shot_number: 2,
        description: "林默走近控制台。".into(),
        camera: "Medium".into(),
        camera_movement: "Slow Push In".into(),
        composition: "Rule of Thirds".into(),
        action: "林默推开椅子站起身，走向控制台。".into(),
        dialogue: String::new(),
        emotion: "警觉".into(),
        duration_seconds: 4,
        environment_id: Some(lab.id.clone()),
        first_frame: None,
        last_frame: None,
        video_model: None,
        generation_settings: String::new(),
        generated_video: None,
        created_at: chrono::Utc::now(),
    };
    db::add_shot(conn, &shot2)?;
    db::link_shot_character(conn, &ShotCharacter {
        shot_id: shot2.id.clone(),
        character_id: lin_mo.id.clone(),
        age_id: age_28.id.clone(),
        appearance_id: app_casual.id.clone(),
    })?;

    let shot3 = Shot {
        id: ShotId::new(),
        scene_id: scene_lab.id.clone(),
        shot_number: 3,
        description: "控制台屏幕特写，能源读数飙升。".into(),
        camera: "Close-up".into(),
        camera_movement: "Slow Push In".into(),
        composition: "Center".into(),
        action: "屏幕上数字快速跳动，红色警示横幅弹出。".into(),
        dialogue: String::new(),
        emotion: "紧张".into(),
        duration_seconds: 3,
        environment_id: Some(lab.id.clone()),
        first_frame: None,
        last_frame: None,
        video_model: None,
        generation_settings: String::new(),
        generated_video: None,
        created_at: chrono::Utc::now(),
    };
    db::add_shot(conn, &shot3)?;

    // Shots — garage scene
    let shot4 = Shot {
        id: ShotId::new(),
        scene_id: scene_garage.id.clone(),
        shot_number: 1,
        description: "林默走入停车场，取出备用手枪。".into(),
        camera: "Medium".into(),
        camera_movement: "Handheld".into(),
        composition: "Center".into(),
        action: "林默从储物柜取出战术外套并穿上。".into(),
        dialogue: String::new(),
        emotion: "警觉".into(),
        duration_seconds: 6,
        environment_id: Some(garage.id.clone()),
        first_frame: None,
        last_frame: None,
        video_model: None,
        generation_settings: String::new(),
        generated_video: None,
        created_at: chrono::Utc::now(),
    };
    db::add_shot(conn, &shot4)?;
    db::link_shot_character(conn, &ShotCharacter {
        shot_id: shot4.id.clone(),
        character_id: lin_mo.id.clone(),
        age_id: age_28.id.clone(),
        appearance_id: app_combat.id.clone(),
    })?;
    db::link_shot_prop(conn, &shot4.id, &core.id)?;

    // Shots — street scene
    let shot5 = Shot {
        id: ShotId::new(),
        scene_id: scene_street.id.clone(),
        shot_number: 1,
        description: "深圳街道远景，灯火依次熄灭。".into(),
        camera: "Wide".into(),
        camera_movement: "Slow Pan".into(),
        composition: "Center".into(),
        action: "街道两侧高楼的灯光逐层熄灭。".into(),
        dialogue: String::new(),
        emotion: "震撼".into(),
        duration_seconds: 8,
        environment_id: Some(street.id.clone()),
        first_frame: None,
        last_frame: None,
        video_model: None,
        generation_settings: String::new(),
        generated_video: None,
        created_at: chrono::Utc::now(),
    };
    db::add_shot(conn, &shot5)?;

    // ---------------- Reference images ----------------
    // Seed two references so the new References section and the inline
    // avatar rows under character / environment / prop cards have
    // something to show on first launch. Sources are empty so the Avatar
    // falls back to initials; the BYOK pipeline is responsible for filling
    // in a real image path later.
    let lin_mo_ref = ReferenceImage {
        id: ReferenceImageId::new(),
        kind: ReferenceKind::Character,
        label: "林默-28岁-参考照".into(),
        source: String::new(),
        created_at: chrono::Utc::now(),
    };
    db::add_reference(conn, &lin_mo_ref, db::ReferenceTarget::CharacterAge(&age_28.id))?;
    db::add_reference(conn, &ReferenceImage {
        id: ReferenceImageId::new(),
        kind: ReferenceKind::Environment,
        label: "AI 实验室-入口".into(),
        source: String::new(),
        created_at: chrono::Utc::now(),
    }, db::ReferenceTarget::Environment(&lab.id))?;
    // Make the character's age default to the seeded reference so the UI
    // has an obvious default to pick.
    conn.execute(
        "UPDATE character_ages SET default_reference = ?1 WHERE id = ?2",
        rusqlite::params![lin_mo_ref.id.as_str(), age_28.id.as_str()],
    )?;

    Ok(())
}


