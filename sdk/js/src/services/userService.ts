export interface UserService {
  getUserAddress(username: string): Promise<string>;
}

export class DefaultUserService implements UserService {
  constructor(private baseURL: string) {}

  async getUserAddress(username: string): Promise<string> {
    try {
      const queryParams = new URLSearchParams({
        userId: username,
      });

      const res = await fetch(`${this.baseURL}/get-user-address?${queryParams}`, {
        method: "GET",
        headers: { "Content-Type": "application/json" },
      });

      if (!res.ok) {
        throw new Error(`HTTP error! status: ${res.status}`);
      }

      const response = await res.json();
      
      if (!response.address) {
        throw new Error("User address not found in response");
      }

      return response.address;
    } catch (error) {
      console.error("Failed to get user address:", error);
      throw new Error(
        `Failed to resolve username "${username}" to address: ${
          error instanceof Error ? error.message : String(error)
        }`
      );
    }
  }
} 