export default class Membership {
  constructor(
    private readonly baseUrl: string,
  ) {}

  /**
   * Delivers/grants an existing membership to a user for DAO participation
   * This method delivers a membership to the specified user, enabling them to become 
   * a member of the decentralized autonomous organization (DAO) and participate in its governance.
   * The membership must already exist and be available for delivery.
   * 
   * @param userId - The unique identifier of the user who will receive the membership
   * @returns Promise resolving to the server response containing membership delivery details
   * @throws Will throw an error if the membership delivery fails or if there's a server error
   */
  public async addOne(userId: string) {
    const res = await fetch(`${this.baseUrl}/add-member`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ userId }),
    });

    const data = await res.json();
    console.log("Add member response:", data);

    if (!res.ok || data.statusCode >= 500) {
      const errorMessage = data.message || `Server error: ${res.status} ${res.statusText}`;
      throw new Error(`Failed to add member: ${errorMessage}`);
    }

    return data;
  }

  /**
   * Checks if a user is a member of the DAO
   * This method verifies whether the specified address corresponds to a current member
   * of the decentralized autonomous organization (DAO). It queries the server to determine
   * the membership status of the given address.
   * 
   * @param address - The blockchain address to check for membership status
   * @returns Promise resolving to a boolean indicating whether the address is a member
   * @throws Will throw an error if the membership check fails or if there's a server error
   */
  public async isMember(address: string) {
    const res = await fetch(`${this.baseUrl}/is-member?address=${address}`, {
      method: "GET",
      headers: { "Content-Type": "application/json" },
    });

    const data = await res.json();
    console.log("Is member response:", data);

    if (!res.ok || data.statusCode >= 500) {
      const errorMessage = data.message || `Server error: ${res.status} ${res.statusText}`;
      throw new Error(`Failed to check member status: ${errorMessage}`);
    }

    return data.ok;
  }


}
