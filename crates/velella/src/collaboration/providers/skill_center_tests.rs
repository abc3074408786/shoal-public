use super::*;
#[test]
fn managed_catalog_uses_current_project_and_never_changes_frozen_role_resources() {
    let root=std::env::temp_dir().join(format!("velella-center-catalog-{}",uuid::Uuid::now_v7()));
    let data=root.join("data");let a=root.join("a");let b=root.join("b");
    let global=data.join("extensions/skill-center/data");
    for path in [&global,a.join(".velella/skills").as_path(),b.join(".velella/skills").as_path()] {
        let package=path.join("packages/sample");std::fs::create_dir_all(&package).unwrap();
        std::fs::write(package.join("SKILL.md"),format!("---\nname: sample\ndescription: fixture\n---\n{}",path.display())).unwrap();
        let digest=skill_digest(&package).unwrap();std::fs::write(path.join("state.json"),serde_json::to_vec(&json!({"version":1,"skills":[{"name":"sample","digest":digest}]})).unwrap()).unwrap();
    }
    std::fs::create_dir_all(data.join("extensions")).unwrap();
    std::fs::write(data.join("extensions/state.json"),r#"{"skill-center":{"enabled":true,"installed":true}}"#).unwrap();
    let catalog=scoped_capabilities(&data,Some(&a)).unwrap();let rows=catalog["skills"].as_array().unwrap();
    assert!(rows.iter().any(|s|s["scope"]=="project"&&s["path"].as_str().unwrap().starts_with(a.to_str().unwrap())));
    assert!(!rows.iter().any(|s|s["path"].as_str().unwrap().starts_with(b.to_str().unwrap())));
    let mut role:RoleDefinition=serde_json::from_value(super::super::orchestration::v2::seed("codex","goal")["roles"][0].clone()).unwrap();
    let chosen=rows.iter().find(|s|s["scope"]=="project").unwrap();
    role.skills=vec![super::super::orchestration::v2::Skill{path:chosen["path"].as_str().unwrap().into(),digest:chosen["digest"].as_str().unwrap().into()}];
    let bindings=freeze_skills(&root.join("role"),&role).unwrap();let captured=Path::new(bindings[0]["path"].as_str().unwrap());let before=std::fs::read(captured).unwrap();
    std::fs::write(a.join(".velella/skills/packages/sample/SKILL.md"),"changed source").unwrap();assert_eq!(std::fs::read(captured).unwrap(),before);assert!(scoped_capabilities(&data,Some(&a)).is_err());
    std::fs::write(data.join("extensions/state.json"),r#"{"skill-center":{"enabled":false}}"#).unwrap();assert!(!scoped_capabilities(&data,Some(&a)).unwrap()["skills"].as_array().unwrap().iter().any(|s|s["source"]=="skill-center"));
    std::fs::remove_dir_all(root).unwrap();
}
