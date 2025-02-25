#[vos::bin]
mod memberships {
    type UserId = [u8; 32];
    type MembershipId = [u8; 32];
    type Role = String;

    #[derive(Default)]
    struct Membership {
        reputation: u16,
    }

    #[derive(Debug)]
    enum Error {
        WrongPermissions,
        MembershipNotOwnedByUser,
    }

    #[vos(storage)]
    #[derive(Default)]
    pub struct Memberships {
        memberships: Map<MembershipId, Membership>,
        user_memberships: Map<UserId, MembershipId>,
        roles: Map<MembershipId, Vec<Role>>,
        update_roles: Set<Role>,
    }

    impl Memberships {
        #[vos(action)]
        pub fn get_membership(&self, user: UserId) -> Option<Membership> {
            self.user_memberships
                .get(user)
                .and_then(|m| self.memberships.get(m))
        }

        #[vos(action)]
        pub fn get_user_roles(&self, user: UserId) -> Option<Vec<Role>> {
            let membership = self.get_membership(user)?;
            self.roles.get(membership.id)
        }

        #[vos(action)]
        pub fn update_reputation(
            &mut self,
            membership: MembershipId,
            reputation: u16,
        ) -> Result<(), Error> {
            if !self.can_update_memberships(self.env().caller()) {
                return Err(Error::WrongPermissions);
            }
            let Some(membership) = self.memberships.get_mut(membership) else {
                return Err(Error::MembershipNotOwnedByUser);
            };
            membership.reputation = reputation;
            Ok(())
        }
    }

    // internal usage
    impl Memberships {
        fn can_update_membership(&self, user: &Caller) -> bool {
            self.update_roles.intersects(user.roles())
        }
    }
}
