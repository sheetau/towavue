import { asset } from "../site/config.mjs";
import { diagonalSize } from "../site/timeline.mjs";

export function MediaThumbnail({ photo, ...props }) {
  const size = diagonalSize(photo.width, photo.height);
  return <img {...props} className="media-thumbnail" src={asset(photo.src)} alt={photo.name} width={photo.width} height={photo.height} draggable="false" style={{ "--image-width": size.width, "--image-height": size.height }} />;
}
