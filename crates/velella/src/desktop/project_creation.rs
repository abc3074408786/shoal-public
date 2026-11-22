use super::*;

fn validate_name(name:&str)->Result<()> {
    let stem=name.split('.').next().unwrap_or("").to_ascii_uppercase();
    anyhow::ensure!(!name.is_empty()&&name.len()<=160&&name.trim()==name&&!name.ends_with('.')&&
        !name.chars().any(|c|c.is_control()||"<>:\"/\\|?*".contains(c))&&name!="."&&name!=".."&&
        !["CON","PRN","AUX","NUL","COM1","COM2","COM3","COM4","COM5","COM6","COM7","COM8","COM9","LPT1","LPT2","LPT3","LPT4","LPT5","LPT6","LPT7","LPT8","LPT9"].contains(&stem.as_str()),
        "请输入有效的项目文件夹名称，不含路径或系统保留名称");
    Ok(())
}

impl Service {
    pub(super) fn create_project_folder(&mut self,req:Value)->Result<Value> {
        let name=req["name"].as_str().context("请输入项目名称")?.trim();
        validate_name(name)?;
        let parent=dunce::canonicalize(req["parent"].as_str().context("请选择保存位置")?).context("保存位置不存在")?;
        anyhow::ensure!(parent.is_dir(),"保存位置必须是文件夹");
        let target=parent.join(name);
        anyhow::ensure!(!self.projects.iter().any(|p|p.direct&&(target.starts_with(&p.path)||p.path.starts_with(&target))),"保存位置位于已打开的普通文件夹项目中，请选择其他位置");
        // Atomic create refuses existing folders, including links; never overwrite.
        std::fs::create_dir(&target).context("目标文件夹已存在或无法创建，请选择新的项目名称")?;
        let initialize=(||->Result<()> {
            git(&target,&["init","-b","main"])?;
            git(&target,&["-c","user.name=Velella","-c","user.email=velella@localhost","-c","commit.gpgsign=false","commit","--allow-empty","-m","Initialize project"])?;
            Ok(())
        })();
        initialize.with_context(||format!("初始化未完成，已保留文件夹 {}。请检查 Git 错误后打开此目录",target.display()))?;
        self.dispatch(json!({"cmd":"project.add","path":target})).with_context(||format!("项目已创建在 {}，登记失败，可使用打开已有文件夹重试",target.display()))
    }
}

#[cfg(test)]mod tests {
    use super::*;
    #[test]fn rejects_paths_and_windows_devices(){
        for name in ["", "..", "../other", "C:\\target", "aux.txt", "COM1", "name.", "a:b", "a\n"] { assert!(validate_name(name).is_err(),"{name}"); }
        assert!(validate_name("我的项目").is_ok());
    }
}
