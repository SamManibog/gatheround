use std::path::PathBuf;

use crate::{
    color::IColor,
    geometry::{IConvexShape, IRect, IVec2},
    resources::GameResources,
};

pub struct EditorSaveState<C: IColor, R: IRect, V: IVec2, S: IConvexShape<V>, T, F, I> {
    /// the name of the project
    pub name: String,

    /// the project's directory
    pub project_directory: PathBuf,

    /// the game's resources
    /// note than any unnamed resources will be discarded when saving
    pub resources: GameResources<C, R, V, S, T, F, I>,
}

impl<C: IColor, R: IRect, V: IVec2, S: IConvexShape<V>, T, F, I>
    EditorSaveState<C, R, V, S, T, F, I>
{
    pub fn new(name: String, project_directory: PathBuf) -> Self {
        Self {
            name,
            project_directory,
            resources: GameResources::default(),
        }
    }

    pub fn load(project_directory: PathBuf) -> Self {
        let _ = project_directory;
        todo!("Implement load editor save state")
    }

    pub fn save(&self) {
        todo!("Implement load editor save state")
    }
}
