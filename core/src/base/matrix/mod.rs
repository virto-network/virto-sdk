mod manager;

pub use manager::*;

// impl<'sdk> AppManager for App<'sdk> {
//   async fn is_installed(&self) -> bool {
//       self.get_room().is_some()
//   }

//   async fn install(&self) -> Result<(), AppManagerError> {
//       if self.is_installed().await {
//           return Err(AppManagerError::AlreadyInstalled);
//       }
//       let mut room = CreateRoomRequest::new();

//       room.visibility = Visibility::Private;
//       room.name = Some(self.get_room_id());
//       room.initial_state =
//           vec![
//               InitialStateEvent::new(RoomEncryptionEventContent::with_recommended_defaults())
//                   .to_raw_any(),
//           ];

//       let room = self
//           .sdk
//           .client()
//           .create_room(room)
//           .await
//           .map_err(|_| AppManagerError::CantInstall("Erro Creating the Room".to_string()))?;

//       Ok(())
//   }

//   async fn uninstall(&self) -> Result<(), AppManagerError> {
//       let room = self.get_room().ok_or(AppManagerError::CantUninstall(
//           "Can't get installed room".to_string(),
//       ))?;

//       room.leave()
//           .await
//           .map_err(|_| AppManagerError::CantUninstall("Can't leave the room".to_string()))?;

//       room.forget()
//           .await
//           .map_err(|_| AppManagerError::CantUninstall("Can't forget the room".to_string()))?;
//       Ok(())
//   }

// }
